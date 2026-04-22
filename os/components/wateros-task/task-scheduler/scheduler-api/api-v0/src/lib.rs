#![no_std]

use task_api::{
    KernelTaskEntry, ScheduleReason, TaskBlockReason, TaskExitCode, TaskId, TaskSnapshot,
    TaskTick, TaskTrapFrame, WaitQueueId,
};

pub trait Scheduler {
    fn init(&mut self);
    fn spawn_kernel_task(&mut self, entry: KernelTaskEntry, arg: usize) -> TaskId;
    fn allocate_wait_queue(&mut self) -> WaitQueueId;
    fn run_first_task(&mut self) -> !;
    fn schedule(&mut self, reason: ScheduleReason);
    fn block_current(&mut self, reason: TaskBlockReason);
    fn wait_current_on(&mut self, wait_queue_id: WaitQueueId);
    fn sleep_current_for_ticks(&mut self, ticks: TaskTick);
    fn wake_task(&mut self, task_id: TaskId) -> bool;
    fn wake_one_in_wait_queue(&mut self, wait_queue_id: WaitQueueId) -> Option<TaskId>;
    fn wake_all_in_wait_queue(&mut self, wait_queue_id: WaitQueueId) -> usize;
    fn exit_current(&mut self, exit_code: TaskExitCode) -> !;
    fn current_task_id(&self) -> Option<TaskId>;
    fn current_task_snapshot(&self) -> Option<TaskSnapshot>;
    fn record_current_trap_frame(&mut self, trap_frame: TaskTrapFrame);
    fn restore_current_trap_frame(&self, trap_frame: &mut TaskTrapFrame) -> bool;
}
