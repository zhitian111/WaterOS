#![no_std]

//! procfs 聚合 crate：按特性选择 `impl-kernel` 或 `impl-dummy` 为 [`active_impl`]。

pub mod api {
    pub use ::api_v0::*;
}

pub use api_v0::*;
#[cfg(feature = "impl-kernel")]
pub use impl_kernel as active_impl;
#[cfg(all(not(feature = "impl-kernel"), feature = "impl-dummy"))]
pub use impl_dummy as active_impl;
