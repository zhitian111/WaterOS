#![no_std]

pub mod api {
    pub use ::api_v0::*;
}

pub mod scheduler {
    pub use ::scheduler::*;
}

#[cfg(feature = "impl-dummy")]
pub use impl_dummy as active_impl;

pub use api_v0::{KernelTask, KernelTaskEntry, TaskContext, TaskId, TaskStatus, IDLE_TASK_ID};

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
pub fn current_task_id() -> Option<TaskId> { scheduler::current_task_id() }
