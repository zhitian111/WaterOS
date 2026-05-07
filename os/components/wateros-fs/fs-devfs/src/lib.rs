#![no_std]

//! 设备文件系统（devfs）聚合 crate：按特性选择 `impl-kernel` 或 `impl-dummy` 为 [`active_impl`]。
//!
//! [`active_impl`] 提供与平台驱动对接的节点刷新、块设备查找及（可选）仅用于展示的 DTB 占位路径。

/// 重导出 devfs API v0。
pub mod api {
    pub use ::api_v0::*;
}

pub use api_v0::*;
#[cfg(feature = "impl-kernel")]
pub use impl_kernel as active_impl;
#[cfg(all(not(feature = "impl-kernel"), feature = "impl-dummy"))]
pub use impl_dummy as active_impl;

