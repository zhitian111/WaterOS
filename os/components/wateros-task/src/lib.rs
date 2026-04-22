#![no_std]

pub mod api {
    pub use ::api_v0::*;
}

pub mod scheduler {
    pub use ::scheduler::*;
}

#[cfg(feature = "impl-dummy")]
pub use impl_dummy as active_impl;

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_schedule_tick() {
    schedule_tick();
}

pub use api_v0::{
    KernelTask, KernelTaskEntry, ScheduleReason, TaskBlockReason, TaskContext, TaskExitCode,
    TaskId, TaskKind, TaskSnapshot, TaskState, TaskTick, IDLE_TASK_ID,
};

#[inline]
pub fn init() { scheduler::init(); }

#[inline]
pub fn spawn_kernel_task(entry: KernelTaskEntry, arg: usize) -> TaskId {
    scheduler::spawn_kernel_task(entry, arg)
}

#[inline]
pub fn run_first_task() -> ! { scheduler::run_first_task() }

#[inline]
pub fn yield_now() { scheduler::suspend_current_and_run_next(); }

#[inline]
pub fn schedule_tick() { scheduler::schedule_tick(); }

#[inline]
pub fn block_current(reason: TaskBlockReason) { scheduler::block_current(reason); }

#[inline]
pub fn sleep_for_ticks(ticks: TaskTick) { scheduler::sleep_current_for_ticks(ticks); }

#[inline]
pub fn wake_task(task_id: TaskId) -> bool { scheduler::wake_task(task_id) }

#[inline]
pub fn exit_current(exit_code: TaskExitCode) -> ! { scheduler::exit_current(exit_code) }

#[inline]
pub fn current_task_id() -> Option<TaskId> { scheduler::current_task_id() }

#[inline]
pub fn current_task_snapshot() -> Option<TaskSnapshot> { scheduler::current_task_snapshot() }
