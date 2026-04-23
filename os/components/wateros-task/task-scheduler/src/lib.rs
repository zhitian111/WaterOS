#![no_std]

pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-dummy")]
pub use impl_dummy as active_impl;

pub use api_v0::Scheduler;
pub use task_api::{
    AddressSpaceHandle, ExitedTask, KernelTaskEntry, ScheduleReason, TaskBlockReason,
    TaskExitCode, TaskId, TaskKind, TaskSnapshot, TaskState, TaskTick, TaskTrapFrame,
    TaskWaitHandle, TaskWaitResult, TaskWaitTarget, UserImageInfo, UserTaskEntryPc,
    UserTaskResources, UserTaskSpec, WaitQueueId, IDLE_TASK_ID,
};

/// 初始化当前启用的调度器实现。
#[inline]
pub fn init() { active_impl::init_scheduler(); }

/// 创建一个新的内核任务，并返回其任务号。
#[inline]
pub fn spawn_kernel_task(entry: KernelTaskEntry, arg: usize) -> TaskId {
    active_impl::spawn_kernel_task(entry, arg)
}

/// 按给定规格创建一个新的用户任务，并返回其任务号。
#[inline]
pub fn spawn_user_task_spec(spec: UserTaskSpec) -> TaskId {
    active_impl::spawn_user_task_spec(spec)
}

/// 创建一个新的最小用户任务骨架，并返回其任务号。
#[inline]
pub fn spawn_user_task(entry_pc: UserTaskEntryPc) -> TaskId {
    spawn_user_task_spec(UserTaskSpec::new(entry_pc))
}

/// 为上层同步对象分配一个等待队列编号。
#[inline]
pub fn allocate_wait_queue() -> WaitQueueId { active_impl::allocate_wait_queue() }

/// 启动调度器并切入第一个可运行任务。
#[inline]
pub fn run_first_task() -> ! { active_impl::run_first_task() }

/// 挂起当前任务并调度下一个可运行任务。
#[inline]
pub fn suspend_current_and_run_next() { active_impl::suspend_current_and_run_next(); }

/// 处理一次时钟 tick 相关的调度逻辑。
#[inline]
pub fn schedule_tick() { active_impl::schedule_tick(); }

/// 按给定原因阻塞当前任务。
#[inline]
pub fn block_current(reason: TaskBlockReason) { active_impl::block_current(reason); }

/// 让当前任务等待指定的阻塞对象。
#[inline]
pub fn wait_current(wait_handle: TaskWaitHandle) { active_impl::wait_current(wait_handle); }

/// 让当前任务等待指定的阻塞对象，并带一个超时。
#[inline]
pub fn wait_current_timeout(
    wait_handle: TaskWaitHandle,
    timeout_ticks: TaskTick,
) -> TaskWaitResult {
    active_impl::wait_current_timeout(wait_handle, timeout_ticks)
}

/// 让当前任务在指定等待队列上休眠，直到被唤醒。
#[inline]
pub fn wait_current_on(wait_queue_id: WaitQueueId) {
    wait_current(TaskWaitHandle::for_wait_queue(wait_queue_id));
}

/// 让当前任务在指定等待队列上等待，并附带超时时间。
#[inline]
pub fn wait_current_on_timeout(
    wait_queue_id: WaitQueueId,
    timeout_ticks: TaskTick,
) -> TaskWaitResult {
    wait_current_timeout(TaskWaitHandle::for_wait_queue(wait_queue_id), timeout_ticks)
}

/// 让当前任务等待指定任务退出。
#[inline]
pub fn wait_for_task_exit(task_id: TaskId) {
    wait_current(TaskWaitHandle::for_task_exit(task_id));
}

/// 让当前任务等待指定任务退出，并附带超时时间。
#[inline]
pub fn wait_for_task_exit_timeout(task_id: TaskId, timeout_ticks: TaskTick) -> TaskWaitResult {
    wait_current_timeout(TaskWaitHandle::for_task_exit(task_id), timeout_ticks)
}

/// 让当前任务睡眠指定数量的 tick。
#[inline]
pub fn sleep_current_for_ticks(ticks: TaskTick) { active_impl::sleep_current_for_ticks(ticks); }

/// 尝试唤醒指定任务。
#[inline]
pub fn wake_task(task_id: TaskId) -> bool { active_impl::wake_task(task_id) }

/// 回收指定已退出任务的退出信息。
#[inline]
pub fn reap_exited_task(task_id: TaskId) -> Option<ExitedTask> { active_impl::reap_exited_task(task_id) }

/// 回收一个任意已退出任务的退出信息。
#[inline]
pub fn reap_one_exited_task() -> Option<ExitedTask> { active_impl::reap_one_exited_task() }

/// 从指定等待队列中唤醒一个任务。
#[inline]
pub fn wake_one_in_wait_queue(wait_queue_id: WaitQueueId) -> Option<TaskId> {
    active_impl::wake_one_in_wait_queue(wait_queue_id)
}

/// 唤醒指定等待队列中的全部任务，并返回唤醒数量。
#[inline]
pub fn wake_all_in_wait_queue(wait_queue_id: WaitQueueId) -> usize {
    active_impl::wake_all_in_wait_queue(wait_queue_id)
}

/// 让当前任务以给定退出码结束运行。
#[inline]
pub fn exit_current(exit_code: TaskExitCode) -> ! { active_impl::exit_current(exit_code) }

/// 返回当前正在运行任务的任务号。
#[inline]
pub fn current_task_id() -> Option<TaskId> { active_impl::current_task_id() }

/// 返回当前正在运行任务的稳定快照。
#[inline]
pub fn current_task_snapshot() -> Option<TaskSnapshot> { active_impl::current_task_snapshot() }

/// 返回当前任务用于处理 trap 的内核栈顶地址。
#[inline]
pub fn current_task_kernel_stack_top() -> Option<usize> {
    active_impl::current_task_kernel_stack_top()
}

/// 将当前 trap 保存现场记录到当前任务对象中。
#[inline]
pub fn record_current_trap_frame(trap_frame: TaskTrapFrame) {
    active_impl::record_current_trap_frame(trap_frame);
}

/// 将当前 trap 现场装载到当前任务对象，并返回权威 trap frame 指针。
#[inline]
pub fn begin_current_trap_frame_access(trap_frame: TaskTrapFrame) -> Option<*mut TaskTrapFrame> {
    active_impl::begin_current_trap_frame_access(trap_frame)
}

/// 将当前任务中保存的 trap 现场恢复到给定缓冲区。
#[inline]
pub fn restore_current_trap_frame(trap_frame: &mut TaskTrapFrame) -> bool {
    active_impl::restore_current_trap_frame(trap_frame)
}
