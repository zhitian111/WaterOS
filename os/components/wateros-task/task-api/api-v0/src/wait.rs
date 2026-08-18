//! 可等待对象与等待结果

use crate::task::{TaskId, WaitQueueId};

/// 带超时等待结束时的结果分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskWaitResult {
    /// 等待对象正常唤醒了任务。
    Woken,
    /// 超时时间先到，任务因超时返回。
    TimedOut,
    /// 信号或其它异步事件中断了等待。
    Interrupted,
}

/// 可被任务等待的目标对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskWaitTarget {
    /// 在这个队列里等待某个条件。
    WaitQueue(WaitQueueId),
    /// 等待某个任务进入退出状态。
    TaskExit(TaskId),
    /// 等待某个父任务的任意子任务进入退出状态；`TaskId` 保存父任务 ID。
    ChildExit(TaskId),
    /// 由内核显式置为阻塞，无特定等待对象。
    Manual,
}
