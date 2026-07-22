// 每 CPU 调度器状态、在线状态与只读查询。

impl MultiClassScheduler {
    pub(super) fn set_cpu_online(&mut self, cpu_id : CpuId) {
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

    pub(super) fn online_cpu_mask(&self) -> base::cpu::CpuMask {
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
        Some(CpuSnapshot { cpu_id,
                           online : cpu.online,
                           current_task_id : cpu.current_task_id,
                           idle_task_id : cpu.idle_task_id,
                           current_address_space : cpu.current_task_id
                                                      .and_then(|id| {
                                                          let raw = self.global
                                                    .registry
                                                    .current_task_address_space_raw(id);
                                                          (raw != 0).then(|| {
                                                              AddressSpaceHandle::from_raw(raw)
                                                          })
                                                      }),
                           current_task_ticks : self.global
                                                    .wait_queues
                                                    .current_tick() })
    }

    /// 返回全部已配置 CPU 的稳定快照，包含尚未 online 的 CPU。
    pub(super) fn cpu_states(&self) -> alloc::vec::Vec<(CpuId, CpuSnapshot)> {
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

    pub(super) fn cpu_load(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].rr_queue
                                     .runnable_count() +
        self.cpu_states[cpu_id.raw()].fifo_queue
                                     .runnable_count() +
        self.cpu_states[cpu_id.raw()].other_queue
                                     .runnable_count()
    }
}
