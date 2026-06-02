#![no_std]
//! Futex dummy 实现占位。
//!
//! 与 `futex-api` 对齐的链接桩：不阻塞、不唤醒；真实实现见 `impl-task`。

mod hub;

pub use hub::FutexHub;

/// impl 层自检：dummy 枢纽可构造且 trait 方法可链接。
pub fn test() {
    use api_v0::KernelFutexOps;

    let hub = FutexHub::new();
    assert_eq!(
        hub.wake(api_v0::FutexKey::from_uaddr(0), 0),
        Err(api_v0::FutexError::Nosys)
    );
    assert!(hub.set_robust_list(1, 0, api_v0::ROBUST_LIST_HEAD_SIZE).is_ok());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impl_smoke() {
        test();
    }
}
