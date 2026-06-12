#![no_std]
//! IPC 视角下的任务等待队列：对 `wateros_task::WaitQueue` 的薄包装，便于 IPC
//! 模块与调度子系统解耦命名。
//!
//! 不变量：不在此类型上附加第二套等待状态；唤醒与 tick 超时语义与
//! `wateros_task` 完全一致。若 IPC 需要额外元数据，应在更高层组合本类型而非扩展
//! `inner`。

/// 任务标识（重导出自 `api-v0`）。
pub use api_v0::TaskId;
/// 调度 tick 类型（重导出自 `api-v0`）。
pub use api_v0::TaskTick;
/// 通用等待句柄（重导出自 `api-v0`）。
pub use api_v0::TaskWaitHandle;
/// 带超时的等待结果（重导出自 `api-v0`）。
pub use api_v0::TaskWaitResult;
/// 等待队列编号类型（重导出自 `api-v0`）。
pub use api_v0::WaitQueueId;

use wateros_task::wait_queue::WaitQueue as TaskWaitQueue;

/// IPC 侧对任务等待队列的薄包装。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitQueue {
    // 仅委托底层队列；不在 IPC 层缓存 waiters 或附加锁。
    inner : TaskWaitQueue,
}

impl WaitQueue {
    /// 创建一个新的 IPC 等待队列。
    #[inline]
    pub fn new() -> Self { Self { inner : TaskWaitQueue::new() } }

    /// 返回该等待队列在底层任务系统中的编号。
    #[inline]
    pub const fn id(&self) -> WaitQueueId { self.inner.id() }

    /// 返回该 IPC 等待队列对应的通用等待句柄。
    #[inline]
    pub const fn wait_handle(&self) -> TaskWaitHandle {
        self.inner
            .wait_handle()
    }

    /// 让当前任务在该 IPC 等待队列上休眠。
    #[inline]
    pub fn wait_current(&self) -> TaskWaitResult {
        self.inner
            .wait_current()
    }

    /// 让当前任务在该 IPC 等待队列上等待，并带一个 tick 级超时。
    #[inline]
    pub fn wait_current_for_ticks(&self, timeout_ticks : TaskTick) -> TaskWaitResult {
        self.inner
            .wait_current_for_ticks(timeout_ticks)
    }

    /// 在调度临界区内复查条件；条件仍成立才让当前任务在该 IPC 等待队列上休眠。
    #[inline]
    pub fn wait_current_while(&self,
                              condition : impl FnOnce() -> bool)
                              -> TaskWaitResult {
        self.inner
            .wait_current_while(condition)
    }

    /// 在调度临界区内复查条件；条件仍成立才让当前任务带超时等待。
    #[inline]
    pub fn wait_current_while_for_ticks(&self,
                                        timeout_ticks : TaskTick,
                                        condition : impl FnOnce() -> bool)
                                        -> TaskWaitResult {
        self.inner
            .wait_current_while_for_ticks(timeout_ticks, condition)
    }

    /// 唤醒一个等待中的任务，并返回被唤醒的任务号。
    #[inline]
    pub fn wake_one(&self) -> Option<TaskId> {
        self.inner
            .wake_one()
    }

    /// 唤醒该等待队列上的全部任务，并返回唤醒数量。
    #[inline]
    pub fn wake_all(&self) -> usize {
        self.inner
            .wake_all()
    }
}

impl Default for WaitQueue {
    #[inline]
    fn default() -> Self { Self::new() }
}

impl api_v0::IpcWaitQueueOps for WaitQueue {
    #[inline]
    fn new() -> Self { Self::new() }

    #[inline]
    fn id(&self) -> WaitQueueId { self.inner.id() }

    #[inline]
    fn wait_handle(&self) -> TaskWaitHandle {
        self.inner
            .wait_handle()
    }

    #[inline]
    fn wait_current(&self) -> TaskWaitResult {
        self.inner
            .wait_current()
    }

    #[inline]
    fn wait_current_for_ticks(&self, timeout_ticks : TaskTick) -> TaskWaitResult {
        self.inner
            .wait_current_for_ticks(timeout_ticks)
    }

    #[inline]
    fn wait_current_while<F>(&self, condition : F) -> TaskWaitResult
        where F : FnOnce() -> bool {
        self.inner
            .wait_current_while(condition)
    }

    #[inline]
    fn wait_current_while_for_ticks<F>(&self,
                                       timeout_ticks : TaskTick,
                                       condition : F)
                                       -> TaskWaitResult
        where F : FnOnce() -> bool
    {
        self.inner
            .wait_current_while_for_ticks(timeout_ticks, condition)
    }

    #[inline]
    fn wake_one(&self) -> Option<TaskId> {
        self.inner
            .wake_one()
    }

    #[inline]
    fn wake_all(&self) -> usize {
        self.inner
            .wake_all()
    }
}
