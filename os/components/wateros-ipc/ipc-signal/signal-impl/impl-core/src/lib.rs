#![no_std]
//! WaterOS 信号状态机实现。
//!
//! `registry` 负责进程/线程状态与投递，`timer` 负责三类 timer；全局锁只在本层出现。

extern crate alloc;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[ipc/signal/impl-core] self_test begin");
    assert!(api_v0::NSIG > 0);
    log::info!("[ipc/signal/impl-core] self_test complete");
}

mod global;
mod registry;
mod state;
mod timer;

pub use global::*;
