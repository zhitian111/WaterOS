#![no_std]
//! IPC 视角下的任务等待队列：对 `wateros_task::WaitQueue` 的薄包装。
//!
//! `ARCH:` 不在此类型上附加第二套等待状态；唤醒、timeout 和 SMP 运行队列语义与
//! `wateros_task` 完全一致。IPC 额外元数据应由对象自己的 registry 持有，不能扩展 `inner`。

/// 任务标识（重导出自 `api-v0`）。
pub use api_v0::TaskId;
/// 调度 tick 类型（重导出自 `api-v0`）。
pub use api_v0::TaskTick;
/// 带超时的等待结果（重导出自 `api-v0`）。
pub use api_v0::TaskWaitResult;
/// 等待目标类型（重导出自 `api-v0`）。
pub use api_v0::TaskWaitTarget;
/// 等待队列编号类型（重导出自 `api-v0`）。
pub use api_v0::WaitQueueId;
pub use wateros_task::wait_queue::WaitQueueRequeueResult;

use wateros_task::wait_queue::WaitQueue as TaskWaitQueue;

/// `DATA:` IPC 侧对任务等待队列的零额外状态包装。
///
/// `SMP:` `wake_*` 只委托 task scheduler；被唤醒任务的目标 CPU 与 IPI 由 scheduler 决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitQueue {
    /// 仅委托底层队列；不在 IPC 层缓存 waiter 或附加锁。
    inner: TaskWaitQueue,
}

impl WaitQueue {
    /// 创建一个新的 IPC 等待队列。
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: TaskWaitQueue::new(),
        }
    }

    /// 创建带静态诊断标签的 IPC 等待队列。
    #[inline]
    pub fn new_named(name: &'static str) -> Self {
        Self {
            inner: TaskWaitQueue::new_named(name),
        }
    }

    /// 返回该等待队列在底层任务系统中的编号。
    #[inline]
    pub const fn id(&self) -> WaitQueueId {
        self.inner.id()
    }

    /// 如果队列当前没有等待者，则释放底层编号供后续等待队列复用。
    #[inline]
    pub fn try_release_empty(&self) -> bool {
        self.inner
            .try_release_empty()
    }

    /// 返回该 IPC 等待队列对应的等待目标。
    #[inline]
    pub const fn wait_target(&self) -> TaskWaitTarget {
        self.inner
            .wait_target()
    }

    /// 让当前任务在该 IPC 等待队列上休眠。
    #[inline]
    pub fn wait_current(&self) -> TaskWaitResult {
        self.inner
            .wait_current()
    }

    /// 让当前任务在该 IPC 等待队列上等待，并带一个 tick 级超时。
    #[inline]
    pub fn wait_current_for_ticks(&self, timeout_ticks: TaskTick) -> TaskWaitResult {
        self.inner
            .wait_current_for_ticks(timeout_ticks)
    }

    /// 在调度临界区内复查条件；条件仍成立才让当前任务在该 IPC 等待队列上休眠。
    #[inline]
    pub fn wait_current_while(&self, condition: impl FnOnce() -> bool) -> TaskWaitResult {
        self.inner
            .wait_current_while(condition)
    }

    /// 在调度临界区内复查条件；条件仍成立才让当前任务带超时等待。
    #[inline]
    pub fn wait_current_while_for_ticks(
        &self,
        timeout_ticks: TaskTick,
        condition: impl FnOnce() -> bool,
    ) -> TaskWaitResult {
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

    /// 定向唤醒一个已由上层 registry 确认属于本对象的任务。
    ///
    /// futex bitset 使用此入口选择匹配 waiter；实际摘队、状态更新和远端 IPI
    /// 仍由 task scheduler 原子完成。
    #[inline]
    pub fn wake_task(&self, task_id: TaskId) -> bool {
        wateros_task::wake_task(task_id)
    }

    /// 唤醒本队列中的部分任务，并把其余等待者迁移到另一个等待队列。
    #[inline]
    pub fn requeue_to(&self, target: Self, wake_count: usize, requeue_count: usize) -> usize {
        self.inner
            .requeue_to(target.inner, wake_count, requeue_count)
    }

    /// 在底层 scheduler 临界区内复查条件后执行迁移。
    #[inline]
    pub fn requeue_to_while(
        &self,
        target: Self,
        wake_count: usize,
        requeue_count: usize,
        condition: impl FnOnce() -> bool,
    ) -> Option<usize> {
        self.inner
            .requeue_to_while(
                target.inner,
                wake_count,
                requeue_count,
                condition,
            )
    }

    /// 条件成立时执行 requeue，并返回 scheduler 验证后的任务集合。
    #[inline]
    pub fn requeue_to_detailed_while(
        &self,
        target: Self,
        wake_count: usize,
        requeue_count: usize,
        condition: impl FnOnce() -> bool,
    ) -> Option<WaitQueueRequeueResult> {
        self.inner
            .requeue_to_detailed_while(target.inner,
                                       wake_count,
                                       requeue_count,
                                       condition)
    }
}

impl Default for WaitQueue {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl api_v0::IpcWaitQueueOps for WaitQueue {
    #[inline]
    fn new() -> Self {
        WaitQueue::new()
    }

    #[inline]
    fn id(&self) -> WaitQueueId {
        self.inner.id()
    }

    #[inline]
    fn wait_target(&self) -> TaskWaitTarget {
        self.inner
            .wait_target()
    }

    #[inline]
    fn wait_current(&self) -> TaskWaitResult {
        self.inner
            .wait_current()
    }

    #[inline]
    fn wait_current_for_ticks(&self, timeout_ticks: TaskTick) -> TaskWaitResult {
        self.inner
            .wait_current_for_ticks(timeout_ticks)
    }

    #[inline]
    fn wait_current_while<F>(&self, condition: F) -> TaskWaitResult
    where
        F: FnOnce() -> bool,
    {
        self.inner
            .wait_current_while(condition)
    }

    #[inline]
    fn wait_current_while_for_ticks<F>(
        &self,
        timeout_ticks: TaskTick,
        condition: F,
    ) -> TaskWaitResult
    where
        F: FnOnce() -> bool,
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

    #[inline]
    fn new_named(name: &'static str) -> Self {
        WaitQueue::new_named(name)
    }

    #[inline]
    fn try_release_empty(&self) -> bool {
        WaitQueue::try_release_empty(self)
    }

    #[inline]
    fn requeue_to(&self, target: Self, wake_count: usize, requeue_count: usize) -> usize {
        WaitQueue::requeue_to(self, target, wake_count, requeue_count)
    }

    #[inline]
    fn requeue_to_while<F>(
        &self,
        target: Self,
        wake_count: usize,
        requeue_count: usize,
        condition: F,
    ) -> Option<usize>
    where
        F: FnOnce() -> bool,
    {
        WaitQueue::requeue_to_while(
            self,
            target,
            wake_count,
            requeue_count,
            condition,
        )
    }
}
