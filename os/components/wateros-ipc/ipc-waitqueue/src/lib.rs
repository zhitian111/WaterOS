#![no_std]

pub use wateros_task::{TaskId, TaskTick, TaskWaitHandle, TaskWaitResult, WaitQueueId};

/// IPC 侧对任务等待队列的薄包装。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitQueue {
    inner: wateros_task::WaitQueue,
}

impl WaitQueue {
    /// 创建一个新的 IPC 等待队列。
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: wateros_task::WaitQueue::new(),
        }
    }

    /// 返回该等待队列在底层任务系统中的编号。
    #[inline]
    pub const fn id(&self) -> WaitQueueId { self.inner.id() }

    /// 返回该 IPC 等待队列对应的通用等待句柄。
    #[inline]
    pub const fn wait_handle(&self) -> TaskWaitHandle { self.inner.wait_handle() }

    /// 让当前任务在该 IPC 等待队列上休眠。
    #[inline]
    pub fn wait_current(&self) { self.inner.wait_current(); }

    /// 让当前任务在该 IPC 等待队列上等待，并带一个 tick 级超时。
    #[inline]
    pub fn wait_current_for_ticks(&self, timeout_ticks: TaskTick) -> TaskWaitResult {
        self.inner.wait_current_for_ticks(timeout_ticks)
    }

    /// 唤醒一个等待中的任务，并返回被唤醒的任务号。
    #[inline]
    pub fn wake_one(&self) -> Option<TaskId> { self.inner.wake_one() }

    /// 唤醒该等待队列上的全部任务，并返回唤醒数量。
    #[inline]
    pub fn wake_all(&self) -> usize { self.inner.wake_all() }
}

impl Default for WaitQueue {
    #[inline]
    fn default() -> Self { Self::new() }
}
