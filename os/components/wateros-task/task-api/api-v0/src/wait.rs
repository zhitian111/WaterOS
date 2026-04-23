use crate::task::{TaskId, WaitQueueId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskWaitResult {
    /// 等待对象正常唤醒了任务。
    Woken,
    /// 超时时间先到，任务因超时返回。
    TimedOut,
}

/// 可被任务等待的目标对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskWaitTarget {
    /// 等待某个显式 wait queue。
    WaitQueue(WaitQueueId),
    /// 等待某个任务进入退出状态。
    TaskExit(TaskId),
}

/// 对一个可等待对象的稳定引用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskWaitHandle {
    target: TaskWaitTarget,
}

impl TaskWaitHandle {
    /// 为指定 wait queue 构造等待句柄。
    #[inline]
    pub const fn for_wait_queue(wait_queue_id: WaitQueueId) -> Self {
        Self {
            target: TaskWaitTarget::WaitQueue(wait_queue_id),
        }
    }

    /// 为指定任务退出事件构造等待句柄。
    #[inline]
    pub const fn for_task_exit(task_id: TaskId) -> Self {
        Self {
            target: TaskWaitTarget::TaskExit(task_id),
        }
    }

    /// 返回该等待句柄指向的目标对象。
    #[inline]
    pub const fn target(&self) -> TaskWaitTarget { self.target }
}
