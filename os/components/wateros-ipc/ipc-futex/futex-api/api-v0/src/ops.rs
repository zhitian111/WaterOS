//! 内核 futex 表契约。

use task_api::TaskId;

use crate::error::{FutexError, FutexResult};
use crate::key::FutexKey;

/// 内核 futex 等待/唤醒表契约。
///
/// 用户内存读写与「检查→睡眠」闭包由 syscall 层（S1）完成；impl 负责队列表与 per-task robust 状态。
pub trait KernelFutexOps: Sized {
    /// 唤醒 `key` 对应队列上最多 `max_wake` 个等待者；`max_wake == 0` 时唤醒 1 个。
    fn wake(&self, key: FutexKey, max_wake: u32) -> FutexResult<usize>;

    /// 唤醒 `key` 对应队列上的全部等待者。
    fn wake_all(&self, key: FutexKey) -> FutexResult<usize>;

    /// 登记 robust 链表头；`len` 须等于 [`crate::ROBUST_LIST_HEAD_SIZE`].
    fn set_robust_list(&self, task: TaskId, head: usize, len: usize) -> FutexResult<()>;

    /// 读取 per-task robust 状态；无登记时返回 `(0, 0)`.
    fn get_robust_list(&self, task: TaskId) -> FutexResult<(usize, usize)>;

    /// 任务退出清理完成后删除 robust 侧表条目。
    fn drop_robust_list(&self, task: TaskId);
}

/// 带条件与可选超时的等待结果（由 `FutexHub::wait_while` 返回）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FutexWaitOutcome {
    /// 正常被唤醒或条件已不成立而返回。
    Woken,
    /// `timeout_ticks` 到期且条件仍成立。
    TimedOut,
}

/// 供 syscall 映射 `FutexWaitOutcome` → `FutexError`。
impl FutexWaitOutcome {
    /// 超时结束时转为 IPC 层错误。
    #[inline]
    pub fn into_result(self) -> FutexResult<()> {
        match self {
            Self::Woken => Ok(()),
            Self::TimedOut => Err(FutexError::TimedOut),
        }
    }
}
