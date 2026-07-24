// 每 CPU 调度器状态、在线状态与只读查询。
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

    pub fn set_cpu_online(&mut self, cpu_id : CpuId) {
        if !cpu_id.fits_capacity(self.cpu_states
                                     .len())
        {
            log::warn!("[cpu] invalid CPU {} ignored",
                       cpu_id.raw());
            return;
        }
        let cpu = &mut self.cpu_states[cpu_id.raw()];
        if cpu.online {
            log::warn!("[cpu] CPU {} already online, ignored",
                       cpu_id.raw());
            return;
        }
        cpu.online = true;
        log::info!("[cpu] CPU {} is now online",
                   cpu_id.raw());
    }

    pub fn online_cpu_mask(&self) -> base::cpu::CpuMask {
        let mut mask = base::cpu::CpuMask::EMPTY;
        for cpu in &self.cpu_states {
            if cpu.online {
                mask.insert(cpu.cpu_id);
            }
        }
        mask
    }

    pub fn cpu_snapshot(&self, cpu_id : CpuId) -> Option<CpuSnapshot> {
        let cpu = self.cpu_states
                      .get(cpu_id.raw())?;
        let current_is_idle = cpu.current_task_id
                                 .is_some_and(|id| {
                                     self.global
                                         .registry
                                         .is_idle(id)
                                 });
        let current_address_space = cpu.current_task_id
                                       .and_then(|id| {
                                           let raw = self.global
                                                         .registry
                                                         .current_task_address_space_raw(id);
                                           (raw != 0).then(|| AddressSpaceHandle::from_raw(raw))
                                       });
        Some(CpuSnapshot { cpu_id,
                           online : cpu.online,
                           current_task_id : cpu.current_task_id,
                           idle_task_id : cpu.idle_task_id,
                           current_is_idle,
                           current_is_user : current_address_space.is_some(),
                           current_address_space,
                           runnable_other : cpu.other_queue
                                               .runnable_count(),
                           runnable_fifo : cpu.fifo_queue
                                              .runnable_count(),
                           runnable_rr : cpu.rr_queue
                                            .runnable_count(),
                           need_resched : cpu.need_resched,
                           context_switches : cpu.context_switches,
                           timer_ticks : cpu.timer_ticks,
                           current_task_ticks : self.global
                                                    .wait_queues
                                                    .current_tick() })
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

    pub fn running_cpu(&self, task_id : TaskId) -> Option<CpuId> {
        self.global
            .registry
            .running_cpu_id(task_id)
    }

    pub fn cpu_load(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].rr_queue
                                     .runnable_count() +
        self.cpu_states[cpu_id.raw()].fifo_queue
                                     .runnable_count() +
        self.cpu_states[cpu_id.raw()].other_queue
                                     .runnable_count()
    }
}
