//! 内核同步原语用的等待队列句柄：封装 `WaitQueueId`，提供 `wait`/`wake` 便捷方法。

use crate::{scheduler, TaskId, TaskTick, TaskWaitResult, TaskWaitTarget, WaitQueueId};
pub use crate::scheduler::WaitQueueRequeueResult;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitQueue {
    /// 调度器内部分配的队列编号。
    id : WaitQueueId,
}

impl WaitQueue {
    /// 创建一个新的等待队列句柄。
    #[inline]
    pub fn new() -> Self { Self::new_named("anonymous") }

    /// 创建带静态诊断标签的等待队列句柄。
    #[inline]
    pub fn new_named(name : &'static str) -> Self {
        Self { id : scheduler::allocate_wait_queue(name) }
    }

    /// 返回该等待队列对应的内部编号。
    #[inline]
    pub const fn id(&self) -> WaitQueueId { self.id }

    /// 如果队列当前没有等待者，则释放底层编号供后续等待队列复用。
    #[inline]
    pub fn try_release_empty(&self) -> bool { scheduler::try_release_wait_queue(self.id) }

    /// 返回该等待队列对应的等待目标。
    #[inline]
    pub const fn wait_target(&self) -> TaskWaitTarget { TaskWaitTarget::WaitQueue(self.id) }

    /// 让当前任务在该等待队列上休眠，直到被显式唤醒。
    #[inline]
    pub fn wait_current(&self) -> TaskWaitResult { scheduler::wait_current(self.wait_target()) }

    /// 让当前任务在该等待队列上等待，超时后返回等待结果。
    #[inline]
    pub fn wait_current_for_ticks(&self, timeout_ticks : TaskTick) -> TaskWaitResult {
        scheduler::wait_current_timeout(self.wait_target(), timeout_ticks)
    }

    /// 在调度临界区内复查条件；条件仍成立才让当前任务在该队列上休眠。
    #[inline]
    pub fn wait_current_while(&self, condition : impl FnOnce() -> bool) -> TaskWaitResult {
        scheduler::wait_current_while(self.wait_target(), condition)
    }

    /// 在调度临界区内复查条件；条件仍成立才让当前任务在该队列上带超时等待。
    #[inline]
    pub fn wait_current_while_for_ticks(&self,
                                        timeout_ticks : TaskTick,
                                        condition : impl FnOnce() -> bool)
                                        -> TaskWaitResult {
        scheduler::wait_current_timeout_while(self.wait_target(),
                                              timeout_ticks,
                                              condition)
    }

    /// 唤醒该等待队列中的一个任务，并返回被唤醒的任务号。
    #[inline]
    pub fn wake_one(&self) -> Option<TaskId> { scheduler::wake_one_in_wait_queue(self.id) }

    /// 唤醒该等待队列中的全部任务，并返回实际唤醒数量。
    #[inline]
    pub fn wake_all(&self) -> usize { scheduler::wake_all_in_wait_queue(self.id) }

    /// 唤醒本队列中的部分任务，并把其余等待者迁移到另一个等待队列。
    #[inline]
    pub fn requeue_to(&self, target : Self, wake_count : usize, requeue_count : usize) -> usize {
        scheduler::requeue_wait_queue(self.id,
                                      target.id,
                                      wake_count,
                                      requeue_count)
    }

    /// 在调度临界区内复查条件；条件成立才执行 wake/requeue。
    #[inline]
    pub fn requeue_to_while(&self,
                            target : Self,
                            wake_count : usize,
                            requeue_count : usize,
                            condition : impl FnOnce() -> bool)
                            -> Option<usize> {
        scheduler::requeue_wait_queue_while(self.id,
                                            target.id,
                                            wake_count,
                                            requeue_count,
                                            condition)
    }

    /// 条件成立时执行 requeue，并返回实际唤醒/迁移的任务 ID。
    #[inline]
    pub fn requeue_to_detailed_while(
        &self,
        target : Self,
        wake_count : usize,
        requeue_count : usize,
        condition : impl FnOnce() -> bool)
        -> Option<WaitQueueRequeueResult> {
        scheduler::requeue_wait_queue_detailed_while(self.id,
                                                      target.id,
                                                      wake_count,
                                                      requeue_count,
                                                      condition)
    }
}
