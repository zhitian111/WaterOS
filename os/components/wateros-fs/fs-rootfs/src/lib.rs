#![no_std]

//! 根文件系统（rootfs）聚合 crate：按特性选择 `impl-kernel` 或 `impl-dummy` 为 [`active_impl`]。
//!
//! 职责边界：维护「当前根卷」共享句柄与根块设备路径；具体 FS 种类由外层注入的 [`fs_api_v0::FsImpl`] 决定。
//!
//! **特性**：与 devfs 聚合 crate 相同，`impl-kernel` 优先于互斥的 `impl-dummy`；默认 `impl-kernel`。

/// 重导出 rootfs API v0。
pub mod api {
    pub use ::api_v0::*;
}

pub use api_v0::*;
#[cfg(feature = "impl-kernel")]
pub use impl_kernel as active_impl;
#[cfg(all(not(feature = "impl-kernel"), feature = "impl-dummy"))]
pub use impl_dummy as active_impl;

