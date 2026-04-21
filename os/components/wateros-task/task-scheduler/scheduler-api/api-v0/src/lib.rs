#![no_std]

use task_api::{KernelTaskEntry, TaskId};

pub trait Scheduler {
    fn init(&mut self);
    fn spawn_kernel_task(&mut self, entry: KernelTaskEntry, arg: usize) -> TaskId;
    fn run_first_task(&mut self) -> !;
    fn suspend_current_and_run_next(&mut self);
    fn schedule_tick(&mut self);
    fn current_task_id(&self) -> Option<TaskId>;
}
