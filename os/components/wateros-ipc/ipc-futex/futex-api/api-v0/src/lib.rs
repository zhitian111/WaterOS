#![no_std]
//! Futex API v0：队列键、错误契约、等待结果与 robust 链表布局。
//!
//! 与 `ipc-futex` 聚合及 `futex-impl` 的边界：稳定类型与语义在此定义；队列表、
//! 用户内存访问与调度阻塞由 impl 提供。syscall 层仅做参数解码与 errno 映射。

mod error;
mod key;
mod robust;
mod wait;

pub use error::{FutexError, FutexResult};
pub use key::{FutexKey, FUTEX_PRIVATE_FLAG};
pub use robust::{
    RobustListHead, RobustListRegistration, FUTEX_OWNER_DIED, FUTEX_TID_MASK, FUTEX_WAITERS,
    ROBUST_LIST_ENTRY_SIZE, ROBUST_LIST_HEAD_SIZE, ROBUST_LIST_LIMIT,
};
pub use wait::FutexWaitOutcome;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robust_layout_matches_linux_64_bit_abi() {
        assert_eq!(ROBUST_LIST_HEAD_SIZE, 24);
    }

    #[test]
    fn wait_outcomes_are_distinct() {
        assert_ne!(FutexWaitOutcome::Woken,
                   FutexWaitOutcome::TimedOut);
        assert_ne!(FutexWaitOutcome::Woken,
                   FutexWaitOutcome::ConditionChanged);
        assert_ne!(FutexWaitOutcome::TimedOut,
                   FutexWaitOutcome::Interrupted);
    }
}
