#![no_std]
//! Futex task 实现：全局 `FutexHub` + `ipc-waitqueue` 阻塞/唤醒 + robust 侧表。

extern crate alloc;

mod hub;
mod robust;

pub use hub::FutexHub;

/// impl 层自检：robust 登记与 wake 空队列。
pub fn test() {
    use api_v0::KernelFutexOps;

    api_v0::test();
    let hub = FutexHub::global();
    assert!(hub
        .set_robust_list(42, 0x1000, api_v0::ROBUST_LIST_HEAD_SIZE)
        .is_ok());
    assert_eq!(
        hub.get_robust_list(42).unwrap(),
        (0x1000, api_v0::ROBUST_LIST_HEAD_SIZE)
    );
    assert_eq!(
        hub.wake(api_v0::FutexKey::from_uaddr(0x2000), 1).unwrap(),
        0
    );
    hub.drop_robust_list(42);
    assert_eq!(hub.get_robust_list(42).unwrap(), (0, 0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impl_smoke() {
        test();
    }
}
