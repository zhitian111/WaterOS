#![no_std]
//! 管道 IPC 聚合：重导出 pipe API v0 与当前启用的实现。
//!
//! 与 `ipc-pipe/pipe-api`、`pipe-impl` 的职责划分：API 层固定错误与结果契约，实现层提供内核内部 ring-buffer pipe。

/// 版本化 pipe API。
pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-dummy")]
/// 当前 pipe 实现命名空间；包名沿用 dummy，行为已替换为真实 ring buffer。
pub use impl_dummy as active_impl;

pub use api_v0::{PipeError, PipeResult, DEFAULT_PIPE_CAPACITY};

#[cfg(feature = "impl-dummy")]
pub use active_impl::{Pipe, PipeEndpoint, PipeEndpointKind};
