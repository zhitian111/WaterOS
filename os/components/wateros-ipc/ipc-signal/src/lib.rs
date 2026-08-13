#![no_std]
//! Signal IPC 聚合层：导出稳定 API 与当前内核实现。
//!
//! 状态机位于 `signal-impl/impl-core`；本层不重复包装实现方法。

/// 版本化信号 API。
pub mod api {
    pub use ::api_v0::*;
}

/// 当前信号实现命名空间。
pub use impl_core as active_impl;

pub use active_impl::*;
pub use api_v0::*;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[ipc/signal] self_test begin");
    impl_core::self_test();
    log::info!("[ipc/signal] self_test complete");
}
