#![no_std]
//! 管道 API v0：定义内核内部 pipe 的错误与结果契约。
//!
//! 与 `ipc-pipe` 聚合及 `pipe-impl` 的边界：用户可见类型与错误在此定义；实现 crate 只依赖本 API 而不反向暴露内核细节。

/// 默认 pipe 缓冲区大小，面向内核自检与早期 IPC 对象。
pub const DEFAULT_PIPE_CAPACITY : usize = 4096;

/// pipe 操作错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeError {
    /// 非阻塞尝试无法立即完成。
    WouldBlock,
    /// 对端已经关闭，继续执行当前方向的 I/O 没有意义。
    BrokenPipe,
    /// pipe 容量为零或不满足实现约束。
    InvalidCapacity,
}

/// pipe 操作结果。
pub type PipeResult<T> = Result<T, PipeError>;
