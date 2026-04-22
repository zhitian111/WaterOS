#![no_std]

pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-dummy")]
pub use impl_dummy as active_impl;

pub use api_v0::Scheduler;
pub use task_api::{
    KernelTask, KernelTaskEntry, ScheduleReason, TaskBlockReason, TaskExitCode, TaskId,
    TaskKind, TaskSnapshot, TaskState, TaskTick, TaskTrapFrame, WaitQueueId, IDLE_TASK_ID,
};

#[inline]
pub fn init() { active_impl::init_scheduler(); }

#[inline]
pub fn spawn_kernel_task(entry: KernelTaskEntry, arg: usize) -> TaskId {
    active_impl::spawn_kernel_task(entry, arg)
}

#[inline]
pub fn allocate_wait_queue() -> WaitQueueId { active_impl::allocate_wait_queue() }

#[inline]
pub fn run_first_task() -> ! { active_impl::run_first_task() }

#[inline]
pub fn suspend_current_and_run_next() { active_impl::suspend_current_and_run_next(); }

#[inline]
pub fn schedule_tick() { active_impl::schedule_tick(); }

#[inline]
pub fn block_current(reason: TaskBlockReason) { active_impl::block_current(reason); }

#[inline]
pub fn wait_current_on(wait_queue_id: WaitQueueId) { active_impl::wait_current_on(wait_queue_id); }

#[inline]
pub fn sleep_current_for_ticks(ticks: TaskTick) { active_impl::sleep_current_for_ticks(ticks); }

#[inline]
pub fn wake_task(task_id: TaskId) -> bool { active_impl::wake_task(task_id) }

#[inline]
pub fn wake_one_in_wait_queue(wait_queue_id: WaitQueueId) -> Option<TaskId> {
    active_impl::wake_one_in_wait_queue(wait_queue_id)
}

#[inline]
pub fn wake_all_in_wait_queue(wait_queue_id: WaitQueueId) -> usize {
    active_impl::wake_all_in_wait_queue(wait_queue_id)
}

#[inline]
pub fn exit_current(exit_code: TaskExitCode) -> ! { active_impl::exit_current(exit_code) }

#[inline]
pub fn current_task_id() -> Option<TaskId> { active_impl::current_task_id() }

#[inline]
pub fn current_task_snapshot() -> Option<TaskSnapshot> { active_impl::current_task_snapshot() }

#[inline]
pub fn record_current_trap_frame(trap_frame: TaskTrapFrame) {
    active_impl::record_current_trap_frame(trap_frame);
}

#[inline]
pub fn restore_current_trap_frame(trap_frame: &mut TaskTrapFrame) -> bool {
    active_impl::restore_current_trap_frame(trap_frame)
}
