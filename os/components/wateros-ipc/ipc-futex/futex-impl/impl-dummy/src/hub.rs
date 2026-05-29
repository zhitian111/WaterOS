//! Dummy futex 表：满足链接与 trait 对象边界，不执行真实等待/唤醒。

use api_v0::{FutexError, FutexKey, FutexResult, KernelFutexOps};

/// 占位 futex 枢纽；所有操作返回 [`FutexError::Nosys`]。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FutexHub;

impl FutexHub {
    /// 创建 dummy futex 枢纽。
    #[inline]
    pub const fn new() -> Self { Self }
}

impl KernelFutexOps for FutexHub {
    #[inline]
    fn wait(&self, _key : FutexKey, _expected : u32) -> FutexResult<()> { Err(FutexError::Nosys) }

    #[inline]
    fn wake(&self, _key : FutexKey, _max_wake : u32) -> FutexResult<usize> { Err(FutexError::Nosys) }

    #[inline]
    fn wake_all(&self, _key : FutexKey) -> FutexResult<usize> { Err(FutexError::Nosys) }
}
