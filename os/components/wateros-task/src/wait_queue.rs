/// 内核同步对象侧分配的等待队列句柄：封装 `WaitQueueId` 并提供 `wait`/`wake`
/// 便捷方法。
use crate::{scheduler, TaskId, TaskTick, TaskWaitHandle, TaskWaitResult, WaitQueueId};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitQueue {
    /// 调度器内部分配的队列编号，与 [`TaskWaitHandle::for_wait_queue`] 一致。
    id : WaitQueueId,
}

impl WaitQueue {
    /// 创建一个新的等待队列句柄。
    #[inline]
    pub fn new() -> Self { Self { id : scheduler::allocate_wait_queue() } }

    /// 返回该等待队列对应的内部编号。
    #[inline]
    pub const fn id(&self) -> WaitQueueId { self.id }

    /// 返回该等待队列对应的通用等待句柄。
    #[inline]
    pub const fn wait_handle(&self) -> TaskWaitHandle { TaskWaitHandle::for_wait_queue(self.id) }

    /// 让当前任务在该等待队列上休眠，直到被显式唤醒。
    #[inline]
    pub fn wait_current(&self) -> TaskWaitResult {
        scheduler::wait_current(self.wait_handle())
    }

    /// 让当前任务在该等待队列上等待，超时后返回等待结果。
    #[inline]
    pub fn wait_current_for_ticks(&self, timeout_ticks : TaskTick) -> TaskWaitResult {
        scheduler::wait_current_timeout(self.wait_handle(), timeout_ticks)
    }

    /// 在调度临界区内复查条件；条件仍成立才让当前任务在该队列上休眠。
    #[inline]
    pub fn wait_current_while(&self,
                              condition : impl FnOnce() -> bool)
                              -> TaskWaitResult {
        scheduler::wait_current_while(self.wait_handle(), condition)
    }

    /// 在调度临界区内复查条件；条件仍成立才让当前任务在该队列上带超时等待。
    #[inline]
    pub fn wait_current_while_for_ticks(&self,
                                        timeout_ticks : TaskTick,
                                        condition : impl FnOnce() -> bool)
                                        -> TaskWaitResult {
        scheduler::wait_current_timeout_while(self.wait_handle(),
                                              timeout_ticks,
                                              condition)
    }

    /// 唤醒该等待队列中的一个任务，并返回被唤醒的任务号。
    #[inline]
    pub fn wake_one(&self) -> Option<TaskId> { scheduler::wake_one_in_wait_queue(self.id) }

    /// 唤醒该等待队列中的全部任务，并返回实际唤醒数量。
    #[inline]
    pub fn wake_all(&self) -> usize { scheduler::wake_all_in_wait_queue(self.id) }
}
