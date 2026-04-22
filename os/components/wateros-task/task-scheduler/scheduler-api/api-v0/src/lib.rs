#![no_std]

use task_api::{
    KernelTaskEntry, ScheduleReason, TaskBlockReason, TaskExitCode, TaskId, TaskSnapshot,
    TaskTick,
};

pub trait Scheduler {
    fn init(&mut self);
    fn spawn_kernel_task(&mut self, entry: KernelTaskEntry, arg: usize) -> TaskId;
    fn run_first_task(&mut self) -> !;
    fn schedule(&mut self, reason: ScheduleReason);
    fn block_current(&mut self, reason: TaskBlockReason);
    fn sleep_current_for_ticks(&mut self, ticks: TaskTick);
    fn wake_task(&mut self, task_id: TaskId) -> bool;
    fn exit_current(&mut self, exit_code: TaskExitCode) -> !;
    fn current_task_id(&self) -> Option<TaskId>;
    fn current_task_snapshot(&self) -> Option<TaskSnapshot>;
}
