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
    /// 等待某个显式 wait queue。
    WaitQueue(WaitQueueId),
    /// 等待某个任务进入退出状态。
    TaskExit(TaskId),
    /// 等待某个父任务的任意子任务进入退出状态。
    ChildExit(TaskId),
}

/// 对一个可等待对象的稳定引用；值语义，可安全复制给调度器入队逻辑。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskWaitHandle {
    /// 该句柄所指的等待目标（队列或任务退出）。
    target : TaskWaitTarget,
}

impl TaskWaitHandle {
    /// 为指定 wait queue 构造等待句柄。
    #[inline]
    pub const fn for_wait_queue(wait_queue_id : WaitQueueId) -> Self {
        Self { target : TaskWaitTarget::WaitQueue(wait_queue_id) }
    }

    /// 为指定任务退出事件构造等待句柄。
    #[inline]
    pub const fn for_task_exit(task_id : TaskId) -> Self {
        Self { target : TaskWaitTarget::TaskExit(task_id) }
    }

    /// 为指定父任务的任意子任务退出事件构造等待句柄。
    #[inline]
    pub const fn for_child_exit(parent_task_id : TaskId) -> Self {
        Self { target : TaskWaitTarget::ChildExit(parent_task_id) }
    }

    /// 返回该等待句柄指向的目标对象。
    #[inline]
    pub const fn target(&self) -> TaskWaitTarget { self.target }
}
