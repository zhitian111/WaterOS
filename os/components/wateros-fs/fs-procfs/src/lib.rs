#![no_std]

//! procfs 聚合 crate：由 `impl-kernel` 提供 [`active_impl`]。
//!
//! 默认 feature 为 `impl-kernel`。

/// 重导出 procfs API v0。
pub mod api {
    pub use ::api_v0::*;
}

pub use api_v0::*;
#[cfg(feature = "impl-kernel")]
pub use impl_kernel as active_impl;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[fs/procfs] self_test begin");
    active_impl::self_test();
    log::info!("[fs/procfs] self_test complete");
}
