#![no_std]
//! 管道 IPC 聚合：导出版本化 API 契约与当前启用的实现类型。
//!
//! - [`api`]：trait、错误与端点方向等稳定契约（`pipe-api/api-v0`）。
//! - [`active_impl`]：当前 feature 选中的实现命名空间。
//! - 根层只导出 fd 可持有的 [`PipeEndpoint`]；共享 ringbuf 对象是实现细节。

/// 版本化 pipe API 契约。
pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-ringbuf")]
/// 当前 pipe 实现命名空间。
pub use impl_ringbuf as active_impl;

pub use api_v0::{
    KernelPipe, PipeEndpointKind, PipeEndpointOps, PipeError, PipeReadFinish, PipeReadLease,
    PipeResult, DEFAULT_PIPE_CAPACITY,
};

#[cfg(feature = "impl-ringbuf")]
pub use active_impl::{NamedPipe, PipeEndpoint};

/// 聚合层自检：串联 API 与当前激活 impl。
#[cfg(feature = "impl-ringbuf")]
pub fn test() {
    api_v0::test();
    active_impl::test();
}
