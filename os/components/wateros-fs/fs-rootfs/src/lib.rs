#![no_std]

//! 根文件系统（rootfs）聚合 crate：由 `impl-kernel` 提供 [`active_impl`]。
//!
//! 职责边界：维护「当前根卷」共享句柄与根块设备路径；具体 FS 种类由外层注入的 [`fs_api_v0::FsImpl`] 决定。
//!
//! 默认 feature 为 `impl-kernel`。

/// 重导出 rootfs API v0。
pub mod api {
    pub use ::api_v0::*;
}

pub use api_v0::*;
#[cfg(feature = "impl-kernel")]
pub use impl_kernel as active_impl;
