#![no_std]
//! Futex task 实现：私有 registry + `ipc-waitqueue` 阻塞/唤醒 + robust 侧表。

extern crate alloc;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[ipc/futex/impl-task] self_test begin");
    assert!(core::mem::size_of::<api_v0::FutexKey>() > 0);
    log::info!("[ipc/futex/impl-task] self_test complete");
}

mod global;
mod registry;

pub use global::*;
