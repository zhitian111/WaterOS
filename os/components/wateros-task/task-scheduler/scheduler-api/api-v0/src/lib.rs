//! 调度器侧 **trait 与调度原因** 抽象：描述实现必须提供的操作集合，与 `task_api` 中的任务类型配合使用。
//!
//! 具体轮转、优先级等算法在 `scheduler-impl` 中实现本模块的 [`Scheduler`]；**不** 定义单任务内存表示（见 `wateros-task-impl-core`）。

#![no_std]

use task_api::{
    ExitedTask, KernelTaskEntry, TaskBlockReason, TaskExitCode, TaskId, TaskSnapshot, TaskTick,
    TaskWaitHandle, TaskWaitResult, UserTaskEntryPc, UserTaskSpec, WaitQueueId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleReason {
    /// 第一次切入任务系统。
    StartFirst,
    /// 当前任务主动让出 CPU。
    Yield,
    /// 由时钟 tick 触发一次调度检查。
    Tick,
    /// 由于阻塞而切换出去。
    Block(TaskBlockReason),
    /// 由于定时睡眠而切换出去。
    Sleep(TaskTick),
    /// 当前任务退出。
    Exit(TaskExitCode),
}

/// 调度器需要对外提供的最小能力集合。
pub trait Scheduler {
    /// 初始化调度器内部状态。
    fn init(&mut self);
    /// 创建一个新的内核任务，并返回其任务号。
    fn spawn_kernel_task(&mut self, entry: KernelTaskEntry, arg: usize) -> TaskId;
    /// 按给定规格创建一个新的用户任务，并返回其任务号。
    fn spawn_user_task_spec(&mut self, spec: UserTaskSpec) -> TaskId;
    /// 创建一个新的最小用户任务骨架，并返回其任务号。
    fn spawn_user_task(&mut self, entry_pc: UserTaskEntryPc) -> TaskId {
        self.spawn_user_task_spec(UserTaskSpec::new(entry_pc))
    }
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
        self.wait_current(TaskWaitHandle::for_wait_queue(
            wait_queue_id,
        ));
    }
    /// 让当前任务在指定等待队列上等待，并带一个超时。
    fn wait_current_on_timeout(
        &mut self,
        wait_queue_id: WaitQueueId,
        timeout_ticks: TaskTick,
    ) -> TaskWaitResult {
        self.wait_current_timeout(
            TaskWaitHandle::for_wait_queue(wait_queue_id),
            timeout_ticks,
        )
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
        self.wait_current_timeout(
            TaskWaitHandle::for_task_exit(task_id),
            timeout_ticks,
        )
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
}
