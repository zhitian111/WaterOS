#![no_std]
//! IPC waitqueue v0 API 契约。
//!
//! 类型别名与 `wateros-task` 调度子系统对齐；IPC 侧仅定义薄包装 trait，不引入第二套等待语义。

/// 任务标识（重导出自 `wateros-task-api-v0`）。
pub use task_api::TaskId;
/// 调度 tick 类型（重导出自 `wateros-task-api-v0`）。
pub use task_api::TaskTick;
/// 等待目标类型（重导出自 `wateros-task-api-v0`）。
pub use task_api::TaskWaitTarget;
/// 带超时的等待结果（重导出自 `wateros-task-api-v0`）。
pub use task_api::TaskWaitResult;
/// 等待队列编号类型（重导出自 `wateros-task-api-v0`）。
pub use task_api::WaitQueueId;

/// IPC 等待队列实现契约；阻塞/唤醒语义与底层 `WaitQueue` 一致。
pub trait IpcWaitQueueOps: Sized {
    /// 创建新的等待队列。
    fn new() -> Self;

    /// 返回队列在任务系统中的编号。
    fn id(&self) -> WaitQueueId;

    /// 返回可用于跨子系统引用的等待目标。
    fn wait_target(&self) -> TaskWaitTarget;

    /// 让当前任务无限期阻塞，直至被唤醒。
    fn wait_current(&self) -> TaskWaitResult;

    /// 让当前任务阻塞，带 tick 级超时。
    fn wait_current_for_ticks(&self, timeout_ticks : TaskTick) -> TaskWaitResult;

    /// 在调度临界区内复查条件；条件仍成立才阻塞。
    fn wait_current_while<F>(&self, condition : F) -> TaskWaitResult
        where F : FnOnce() -> bool;

    /// 带超时的条件等待。
    fn wait_current_while_for_ticks<F>(&self,
                                       timeout_ticks : TaskTick,
                                       condition : F)
                                       -> TaskWaitResult
        where F : FnOnce() -> bool;

    /// 唤醒一个等待者，返回被唤醒任务号。
    fn wake_one(&self) -> Option<TaskId>;

    /// 唤醒全部等待者，返回唤醒数量。
    fn wake_all(&self) -> usize;
}
