#![no_std]

//! 设备文件系统（devfs）聚合 crate：按特性选择 `impl-kernel` 或 `impl-dummy` 为 [`active_impl`]。
//!
//! [`active_impl`] 提供与平台驱动对接的节点刷新、块设备查找及（可选）仅用于展示的 DTB 占位路径。
//!
//! **特性**：同时启用 `impl-kernel` 与 `impl-dummy` 时优先 `impl-kernel`；默认 feature 为 `impl-kernel`（见本 crate `Cargo.toml`）。占位 dummy 用于无驱动或最小链接场景。

/// 重导出 devfs API v0。
pub mod api {
    pub use ::api_v0::*;
}

pub use api_v0::*;
#[cfg(feature = "impl-kernel")]
pub use impl_kernel as active_impl;
#[cfg(all(not(feature = "impl-kernel"), feature = "impl-dummy"))]
pub use impl_dummy as active_impl;

