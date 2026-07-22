use api_v0::{CpuId, CpuMask, TaskId};

/// 返回指定 CPU 的调度状态快照。
pub fn cpu_snapshot(cpu_id : CpuId) -> Option<scheduler::CpuSnapshot> {
    scheduler::cpu_snapshot(cpu_id)
}
/// 查询指定任务当前在哪个 CPU 上运行。
pub fn running_cpu(task_id : TaskId) -> Option<CpuId> { scheduler::running_cpu(task_id) }
/// 将指定 CPU 标记为 online。AP 完成初始化后调用。
pub fn set_cpu_online(cpu_id : CpuId) { scheduler::set_cpu_online(cpu_id) }
/// Snapshot of CPUs that have completed task-scheduler bring-up.
pub fn online_cpu_mask() -> CpuMask { scheduler::online_cpu_mask() }
pub fn print_cpu_states() { scheduler::print_cpu_states(); }
