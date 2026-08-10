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
                           boot_context_active : cpu.boot_context_active,
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

    /// 判断某 CPU 是否负载偏高（高于系统平均负载 + 1 视为过载）。
    /// 用于唤醒亲和性放宽：last_cpu 过载时，把任务放到更空的核。
    ///
    /// 额外规则：只要有核空闲，任何非零负载的核都视为过载，
    /// 避免出现"一个核忙、其他核空转"的积压。
    pub(super) fn cpu_is_overloaded(&self, cpu_id : CpuId) -> bool {
        let load = self.cpu_states[cpu_id.raw()].load();
        if load == 0 {
            return false;
        }
        // 有核空闲 → 当前核过载，优先把任务放到空闲核
        for cpu in &self.cpu_states {
            if cpu.online && cpu.load() == 0 {
                return true;
            }
        }
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
        load > total / online + 1
    }
}
