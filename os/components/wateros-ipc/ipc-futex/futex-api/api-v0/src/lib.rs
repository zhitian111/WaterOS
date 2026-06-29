#![no_std]
//! Futex API v0：队列键、错误契约、内核 futex 表 trait 与 robust 链表布局。
//!
//! 与 `ipc-futex` 聚合及 `futex-impl` 的边界：稳定类型与语义在此定义；队列表、
//! 用户内存访问与调度阻塞由 impl 提供。syscall 层仅做参数解码与 errno 映射。

mod error;
mod key;
mod ops;
mod robust;

pub use error::{FutexError, FutexResult};
pub use key::{FutexKey, FUTEX_PRIVATE_FLAG};
pub use ops::{FutexWaitOutcome, KernelFutexOps};
pub use robust::{
    RobustListHead, FUTEX_OWNER_DIED, FUTEX_TID_MASK, ROBUST_LIST_ENTRY_SIZE, ROBUST_LIST_HEAD_SIZE,
    ROBUST_LIST_LIMIT,
};

/// API 层自检：校验 robust 头大小与错误枚举可比较。
#[inline]
pub fn test() {
    assert_eq!(ROBUST_LIST_HEAD_SIZE, 24);
    assert_eq!(FutexError::Again, FutexError::Again);
    assert_eq!(FutexError::TimedOut, FutexError::TimedOut);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_smoke() {
        test();
    }
}
