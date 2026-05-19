#![no_std]
//! IPC waitqueue v0 API 契约。

/// 任务标识（重导出自 `wateros-task-api-v0`）。
pub use task_api::TaskId;
/// 调度 tick 类型（重导出自 `wateros-task-api-v0`）。
pub use task_api::TaskTick;
/// 通用等待句柄（重导出自 `wateros-task-api-v0`）。
pub use task_api::TaskWaitHandle;
/// 带超时的等待结果（重导出自 `wateros-task-api-v0`）。
pub use task_api::TaskWaitResult;
/// 等待队列编号类型（重导出自 `wateros-task-api-v0`）。
pub use task_api::WaitQueueId;

/// IPC 等待队列实现契约。
pub trait IpcWaitQueueOps: Sized {
    fn new() -> Self;

    fn id(&self) -> WaitQueueId;

    fn wait_handle(&self) -> TaskWaitHandle;

    fn wait_current(&self);

    fn wait_current_for_ticks(&self, timeout_ticks : TaskTick) -> TaskWaitResult;

    fn wait_current_while<F>(&self, condition : F)
        where F : FnOnce() -> bool;

    fn wait_current_while_for_ticks<F>(&self,
                                       timeout_ticks : TaskTick,
                                       condition : F)
                                       -> TaskWaitResult
        where F : FnOnce() -> bool;

    fn wake_one(&self) -> Option<TaskId>;

    fn wake_all(&self) -> usize;
}
