#![no_std]

use task_api::{
    ExitedTask, KernelTaskEntry, ScheduleReason, TaskBlockReason, TaskExitCode, TaskId,
    TaskSnapshot, TaskTick, TaskTrapFrame, TaskWaitHandle, TaskWaitResult, WaitQueueId,
};

/// 调度器需要对外提供的最小能力集合。
pub trait Scheduler {
    /// 初始化调度器内部状态。
    fn init(&mut self);
    /// 创建一个新的内核任务，并返回其任务号。
    fn spawn_kernel_task(&mut self, entry: KernelTaskEntry, arg: usize) -> TaskId;
    /// 分配一个新的等待队列编号。
    fn allocate_wait_queue(&mut self) -> WaitQueueId;
    /// 启动调度器并切入第一批任务。
    fn run_first_task(&mut self) -> !;
    /// 按给定原因执行一次调度决策。
    fn schedule(&mut self, reason: ScheduleReason);
    /// 将当前任务标记为阻塞，并切换到其他任务。
    fn block_current(&mut self, reason: TaskBlockReason);
    /// 让当前任务等待指定的阻塞对象。
    fn wait_current(&mut self, wait_handle: TaskWaitHandle);
    /// 让当前任务等待指定的阻塞对象，并带一个超时。
    fn wait_current_timeout(
        &mut self,
        wait_handle: TaskWaitHandle,
        timeout_ticks: TaskTick,
    ) -> TaskWaitResult;
    /// 让当前任务在指定等待队列上休眠。
    fn wait_current_on(&mut self, wait_queue_id: WaitQueueId) {
        self.wait_current(TaskWaitHandle::for_wait_queue(wait_queue_id));
    }
    /// 让当前任务在指定等待队列上等待，并带一个超时。
    fn wait_current_on_timeout(
        &mut self,
        wait_queue_id: WaitQueueId,
        timeout_ticks: TaskTick,
    ) -> TaskWaitResult {
        self.wait_current_timeout(TaskWaitHandle::for_wait_queue(wait_queue_id), timeout_ticks)
    }
    /// 让当前任务等待指定任务退出。
    fn wait_for_task_exit(&mut self, task_id: TaskId) {
        self.wait_current(TaskWaitHandle::for_task_exit(task_id));
    }
    /// 让当前任务等待指定任务退出，并带一个超时。
    fn wait_for_task_exit_timeout(
        &mut self,
        task_id: TaskId,
        timeout_ticks: TaskTick,
    ) -> TaskWaitResult {
        self.wait_current_timeout(TaskWaitHandle::for_task_exit(task_id), timeout_ticks)
    }
    /// 让当前任务睡眠指定 tick 数。
    fn sleep_current_for_ticks(&mut self, ticks: TaskTick);
    /// 尝试唤醒指定任务，成功返回 `true`。
    fn wake_task(&mut self, task_id: TaskId) -> bool;
    /// 回收指定已退出任务的退出信息。
    fn reap_exited_task(&mut self, task_id: TaskId) -> Option<ExitedTask>;
    /// 回收一个任意已退出任务的退出信息。
    fn reap_one_exited_task(&mut self) -> Option<ExitedTask>;
    /// 从指定等待队列中唤醒一个任务。
    fn wake_one_in_wait_queue(&mut self, wait_queue_id: WaitQueueId) -> Option<TaskId>;
    /// 唤醒指定等待队列中的全部任务，并返回唤醒数量。
    fn wake_all_in_wait_queue(&mut self, wait_queue_id: WaitQueueId) -> usize;
    /// 让当前任务退出，不再返回。
    fn exit_current(&mut self, exit_code: TaskExitCode) -> !;
    /// 读取当前正在运行任务的任务号。
    fn current_task_id(&self) -> Option<TaskId>;
    /// 读取当前正在运行任务的稳定快照。
    fn current_task_snapshot(&self) -> Option<TaskSnapshot>;
    /// 记录当前任务最近一次 trap 的保存现场。
    fn record_current_trap_frame(&mut self, trap_frame: TaskTrapFrame);
    /// 将当前 trap 现场装载到当前任务对象，并返回权威 trap frame 指针。
    fn begin_current_trap_frame_access(&mut self, trap_frame: TaskTrapFrame) -> Option<*mut TaskTrapFrame>;
    /// 将当前任务保存的 trap 现场恢复到给定缓冲区。
    fn restore_current_trap_frame(&self, trap_frame: &mut TaskTrapFrame) -> bool;
}
