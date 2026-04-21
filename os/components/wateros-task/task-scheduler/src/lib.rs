#![no_std]

pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-dummy")]
pub use impl_dummy as active_impl;

pub use api_v0::Scheduler;
pub use task_api::{KernelTask, KernelTaskEntry, TaskContext, TaskId, TaskStatus, IDLE_TASK_ID};

#[inline]
pub fn init() { active_impl::init_scheduler(); }

#[inline]
pub fn spawn_kernel_task(entry: KernelTaskEntry, arg: usize) -> TaskId {
    active_impl::spawn_kernel_task(entry, arg)
}

#[inline]
pub fn run_first_task() -> ! { active_impl::run_first_task() }

#[inline]
pub fn suspend_current_and_run_next() { active_impl::suspend_current_and_run_next(); }

#[inline]
pub fn schedule_tick() { active_impl::schedule_tick(); }

#[inline]
pub fn current_task_id() -> Option<TaskId> { active_impl::current_task_id() }
