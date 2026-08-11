// 只读查询：当前任务/指定任务快照、状态、tick 等。
use super::*;
impl MultiClassScheduler {
    pub fn current_task_id(&self, cpu_id : CpuId) -> Option<TaskId> {
        self.cpu_states[cpu_id.raw()].current_task_id()
    }

    pub fn current_task_snapshot(&self, cpu_id : CpuId) -> Option<TaskSnapshot> {
        let cpu = &self.cpu_states[cpu_id.raw()];
        let mut snapshot = self.registry
                               .task_snapshot(cpu.current_task_id()?);
        // Running-task accounting is cached per CPU and written back to the
        // TCB only when the task leaves the CPU. Observers such as getrusage
        // must include the live delta even when the task has not switched.
        let live_ticks = usize::try_from(cpu.current_runtime_ticks).unwrap_or(usize::MAX);
        snapshot.stats
                .tick_count = snapshot.stats
                                      .tick_count
                                      .saturating_add(live_ticks);
        if CPUState::is_cfs_policy(cpu.current_policy()) {
            snapshot.vruntime = cpu.current_vruntime();
        }
        Some(snapshot)
    }

    pub fn current_aspace(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].current_aspace()
    }

    pub fn task_snapshot(&self, task_id : TaskId) -> TaskSnapshot {
        self.registry
            .task_snapshot(task_id)
    }

    pub fn task_state(&self, task_id : TaskId) -> Option<TaskState> {
        self.registry
            .state(task_id)
    }

    pub fn diagnostic_task_snapshots(&self) -> alloc::vec::Vec<TaskSnapshot> {
        self.registry
            .diagnostic_task_snapshots()
    }

    pub fn has_child(&self, parent_id : TaskId) -> bool {
        self.registry
            .has_child(parent_id)
    }

    pub fn current_tick(&self) -> TaskTick {
        self.wait_queues
            .current_tick()
    }
}
