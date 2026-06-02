//! Dummy futex 表：满足链接与 trait 对象边界，不执行真实等待/唤醒。

use api_v0::{FutexError, FutexKey, FutexResult, KernelFutexOps};
use task_api::TaskId;

/// 占位 futex 枢纽；等待/唤醒返回 [`FutexError::Nosys`]。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FutexHub;

impl FutexHub {
    /// 创建 dummy futex 枢纽。
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// 全局单例（dummy 与 task impl 签名一致，便于链接）。
    #[inline]
    pub fn global() -> &'static Self {
        &Self::PLACEHOLDER
    }
}

static PLACEHOLDER: FutexHub = FutexHub;

impl KernelFutexOps for FutexHub {
    #[inline]
    fn wake(&self, _key: FutexKey, _max_wake: u32) -> FutexResult<usize> {
        Err(FutexError::Nosys)
    }

    #[inline]
    fn wake_all(&self, _key: FutexKey) -> FutexResult<usize> {
        Err(FutexError::Nosys)
    }

    #[inline]
    fn set_robust_list(&self, _task: TaskId, _head: usize, len: usize) -> FutexResult<()> {
        if len != api_v0::ROBUST_LIST_HEAD_SIZE {
            return Err(FutexError::Invalid);
        }
        Ok(())
    }

    #[inline]
    fn get_robust_list(&self, _task: TaskId) -> FutexResult<(usize, usize)> {
        Ok((0, 0))
    }

    #[inline]
    fn drop_robust_list(&self, _task: TaskId) {}
}
