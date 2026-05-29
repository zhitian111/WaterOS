#![no_std]
//! Futex dummy 实现占位。
//!
//! 与 `futex-api` 对齐前的链接桩：不阻塞、不唤醒、不遍历 robust 链表；
//! 真实实现（如基于 `ipc-waitqueue` 的 `impl-task`）接入后在此替换。

mod hub;

pub use hub::FutexHub;

/// impl 层自检：dummy 枢纽可构造且 trait 方法可链接。
pub fn test() {
    use api_v0::KernelFutexOps;

    let hub = FutexHub::new();
    assert_eq!(
        hub.wait(api_v0::FutexKey::from_uaddr(0), 0),
        Err(api_v0::FutexError::Nosys)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impl_smoke() {
        test();
    }
}
