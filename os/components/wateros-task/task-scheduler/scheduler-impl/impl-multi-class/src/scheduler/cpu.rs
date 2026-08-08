// 每 CPU 调度器状态、在线状态、只读查询与负载均衡（空闲偷取）。
use super::*;
impl MultiClassScheduler {
    pub fn set_timekeeper_cpu(&mut self, cpu_id : CpuId) {
        assert!(cpu_id.fits_capacity(self.cpu_states
                                         .len()),
                "invalid scheduler timekeeper CPU {}",
                cpu_id.raw());
        if let Some(previous) = self.timekeeper_cpu
                                    .replace(cpu_id)
        {
            assert_eq!(previous,
                       cpu_id,
                       "scheduler timekeeper changed from CPU {} to CPU {}",
                       previous.raw(),
                       cpu_id.raw());
            return;
        }
        log::info!("[scheduler] CPU {} is global timekeeper",
                   cpu_id.raw());
    }
    #[inline]
    pub fn is_timekeeper_cpu(&self, cpu_id : CpuId) -> bool { self.timekeeper_cpu == Some(cpu_id) }

    /// 启动栈尚未通过 `run_first_task` 切出时，不能把预置的 idle cache 当作
    /// 当前硬件上下文执行调度。
    #[inline]
    pub(crate) fn boot_context_active(&self, cpu_id : CpuId) -> bool {
        self.cpu_states[cpu_id.raw()].boot_context_active
    }

    pub fn set_cpu_online(&mut self, cpu_id : CpuId) {
        if !cpu_id.fits_capacity(self.cpu_states
                                     .len())
        {
            log::warn!("[cpu] invalid CPU {} ignored",
                       cpu_id.raw());
            return;
        }
        let cpu = &mut self.cpu_states[cpu_id.raw()];
        if cpu.online() {
            log::warn!("[cpu] CPU {} already online, ignored",
                       cpu_id.raw());
            return;
        }
        cpu.set_online(true);
    }

    pub fn online_cpu_mask(&self) -> CpuMask {
        let mut mask = CpuMask::EMPTY;
        for cpu in &self.cpu_states {
            if cpu.online() {
                mask.insert(cpu.cpu_id);
            }
        }
        mask
    }

    pub fn cpu_snapshot(&self, cpu_id : CpuId) -> Option<CpuSnapshot> {
        let cpu = self.cpu_states
                      .get(cpu_id.raw())?;
        let current_is_idle = cpu.current_task_id() == cpu.idle_task_id;
        let current_address_space = cpu.current_task_id()
                                       .and_then(|id| {
                                           let raw = self.registry
                                                         .current_task_address_space_raw(id);
                                           (raw != 0).then(|| AddressSpaceHandle::from_raw(raw))
                                       });
        Some(CpuSnapshot { cpu_id,
                           online : cpu.online(),
                           current_task_id : cpu.current_task_id(),
                           idle_task_id : cpu.idle_task_id,
                           current_is_idle,
                           current_is_user : current_address_space.is_some(),
                           current_address_space,
                           runnable_other : cpu.cfs_queue
                                               .task_count(),
                           runnable_batch : cpu.batch_queue
                                               .task_count(),
                           runnable_fifo : cpu.fifo_queue
                                              .task_count(),
                           runnable_rr : cpu.rr_queue
                                            .task_count(),
                           runnable_idle : cpu.idle_queue
                                              .task_count(),
                           need_resched : cpu.need_resched,
                           context_switches : cpu.context_switches,
                           timer_ticks : cpu.timer_ticks,
                           idle_ticks : cpu.idle_ticks,
                           current_ticks : cpu.current_ticks })
    }

    /// 返回全部已配置 CPU 的稳定快照，包含尚未 online 的 CPU。
    pub fn cpu_states(&self) -> alloc::vec::Vec<(CpuId, CpuSnapshot)> {
        let mut states = alloc::vec::Vec::with_capacity(self.cpu_states
                                                            .len());
        for cpu in &self.cpu_states {
            let snapshot = self.cpu_snapshot(cpu.cpu_id)
                               .expect("configured CPU must have a snapshot");
            states.push((cpu.cpu_id, snapshot));
        }
        states
    }

    pub fn total_idle_ticks(&self) -> u64 {
        self.cpu_states
            .iter()
            .filter(|cpu| cpu.online())
            .fold(0u64, |total, cpu| {
                total.saturating_add(cpu.idle_ticks)
            })
    }

    pub fn running_cpu(&self, task_id : TaskId) -> Option<CpuId> {
        self.registry
            .running_cpu_id(task_id)
    }

    /// 若任务正在某个 CPU 上运行，请求该 CPU 尽快进入调度安全点。
    pub fn request_task_reschedule(&mut self, task_id : TaskId) {
        if let Some(cpu_id) = self.running_cpu(task_id) {
            self.request_reschedule(cpu_id, RescheduleCause::Forced);
        }
    }

    pub fn cpu_load(&self, cpu_id : CpuId) -> usize { self.cpu_states[cpu_id.raw()].load() }

    // ================================================================
    //  负载均衡：空闲偷取（idle pull）+ 唤醒亲和性放宽
    // ================================================================

    /// 判断某 CPU 是否负载偏高（高于系统平均负载 + 1 视为过载）。
    /// 用于唤醒亲和性放宽：last_cpu 过载时，把任务放到更空的核。
    pub(super) fn cpu_is_overloaded(&self, cpu_id : CpuId) -> bool {
        let mut online = 0usize;
        let mut total = 0usize;
        for cpu in &self.cpu_states {
            if cpu.online {
                online += 1;
                total += cpu.load();
            }
        }
        if online == 0 {
            return false;
        }
        self.cpu_states[cpu_id.raw()].load() > total / online + 1
    }

    /// 选择本 CPU 下一个可运行任务；本地无任务可跑时尝试从其它核偷取，
    /// 避免出现“有的核在排队、有的核空转”的负载失衡。
    pub(super) fn pick_next_runnable_or_steal(&mut self, cpu_id : CpuId) -> TaskId {
        let next = self.cpu_states[cpu_id.raw()].pick_next_runnable();
        let idle_id = self.cpu_states[cpu_id.raw()].idle_task_id
                                                   .expect("every CPU must have an idle task");
        if next != idle_id {
            return next;
        }
        if self.steal_ready_task(cpu_id)
               .is_some()
        {
            return self.cpu_states[cpu_id.raw()].pick_next_runnable();
        }
        idle_id
    }

    /// 从负载最重的其它 online CPU 偷取一个可运行任务并迁到本 CPU。
    /// 源核 load 至少为 2（一个正在运行 + 一个可偷）才值得偷，既不会偷走
    /// 源核唯一的正在运行任务，也能避免任务在两核间来回震荡。
    pub(super) fn steal_ready_task(&mut self, cpu_id : CpuId) -> Option<TaskId> {
        // 本 CPU 已有可运行任务时不偷取。
        if self.cpu_states[cpu_id.raw()].load() > 0 {
            return None;
        }
        let mut busiest = None;
        let mut busiest_load = 1usize;
        for (index, cpu) in self.cpu_states
                                .iter()
                                .enumerate()
        {
            let other = CpuId::from_raw(index);
            if other == cpu_id || !cpu.online {
                continue;
            }
            let load = cpu.load();
            if load > busiest_load {
                busiest_load = load;
                busiest = Some(other);
            }
        }
        let src = busiest?;
        // 从最忙核挑一个可迁移到本核的任务；affinity 由调用侧判定。
        let task_id = self.cpu_states[src.raw()].steal_candidate(|task_id| {
                                                    self.registry
                                                        .task_snapshot(task_id)
                                                        .affinity
                                                        .contains(cpu_id)
                                                })?;
        // 防御：偷取的任务绝不能是任何 CPU 的 current（"就绪但仍在跑"=竞态脏状态）。
        let running_elsewhere = self.cpu_states
                                    .iter()
                                    .any(|cpu| cpu.current_task_id() == Some(task_id));
        if running_elsewhere {
            log::warn!("[sched-steal] task {} is still running on another CPU; skip steal",
                       task_id);
            return None;
        }
        log::debug!("[sched-steal] cpu={} stole task={} from cpu={}",
                    cpu_id.raw(),
                    task_id,
                    src.raw());
        self.enqueue_ready_on_cpu(task_id, cpu_id);
        Some(task_id)
    }
}
