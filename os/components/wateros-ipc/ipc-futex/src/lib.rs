#![no_std]
//! Futex IPC 聚合层：导出稳定 API 与 task-backed 实现。
//!
//! registry 与锁只存在于实现层；调用方直接使用模块级 futex 操作。

/// 版本化 futex API 契约。
pub mod api {
    pub use ::api_v0::*;
}

/// 当前 futex 实现命名空间。
pub use impl_task as active_impl;

pub use active_impl::*;
pub use api_v0::*;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[ipc/futex] self_test begin");
    impl_task::self_test();
    log::info!("[ipc/futex] self_test complete");
}
