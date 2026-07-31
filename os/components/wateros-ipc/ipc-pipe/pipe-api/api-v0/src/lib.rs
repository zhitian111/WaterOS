#![no_std]
//! 管道 API v0：错误、端点方向与内核 pipe / fd 端点 trait 契约。
//!
//! `ARCH:` 稳定类型与 I/O 语义在此定义；ring buffer、端点引用与等待队列逻辑由 impl 提供。
//! API 不管理 fd 表、任务阻塞或 IPI。默认缓冲区容量来自 `wateros-base-config::ipc`。

mod endpoint;
mod error;
mod kernel_pipe;

pub use endpoint::{PipeEndpointKind, PipeEndpointOps, PipeReadFinish, PipeReadLease};
pub use error::{PipeError, PipeResult, DEFAULT_PIPE_CAPACITY};
pub use kernel_pipe::KernelPipe;

/// API 层自检：校验默认容量与错误枚举可比较。
pub fn test() {
    assert_ne!(DEFAULT_PIPE_CAPACITY, 0);
    assert_eq!(
        PipeError::WouldBlock,
        PipeError::WouldBlock
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_smoke() {
        test();
    }
}
