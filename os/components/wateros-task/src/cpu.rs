use api_v0::{CpuId, CpuMask, TaskId};

/// 返回指定 CPU 的调度状态快照。
pub fn cpu_snapshot(cpu_id : CpuId) -> Option<scheduler::CpuSnapshot> {
    scheduler::cpu_snapshot(cpu_id)
}

/// 获取全部已配置 CPU 的一次性调度快照。
///
/// 由 scheduler 在同一把锁下收集，适合状态面板等需要比较多个 CPU 的观察者；
/// 包含尚未 online 的 CPU。
pub fn cpu_states() -> alloc::vec::Vec<(CpuId, scheduler::CpuSnapshot)> {
    scheduler::cpu_states()
}
/// 查询指定任务当前在哪个 CPU 上运行。
pub fn running_cpu(task_id : TaskId) -> Option<CpuId> { scheduler::running_cpu(task_id) }
/// 将指定 CPU 标记为 online。AP 完成初始化后调用。
pub fn set_cpu_online(cpu_id : CpuId) { scheduler::set_cpu_online(cpu_id) }
/// Snapshot of CPUs that have completed task-scheduler bring-up.
pub fn online_cpu_mask() -> CpuMask { scheduler::online_cpu_mask() }
pub fn print_cpu_states() { scheduler::print_cpu_states(); }
