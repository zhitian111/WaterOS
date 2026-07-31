//! pipe 错误与结果类型。

/// 默认 pipe 缓冲区大小；数值定义于 [`wateros_base_config::ipc`]。
pub use wateros_base_config::ipc::DEFAULT_PIPE_CAPACITY;

/// pipe 操作错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeError {
    /// 非阻塞尝试无法立即完成。
    WouldBlock,
    /// 阻塞读写被异步信号中断。
    Interrupted,
    /// 对端已经关闭，继续执行当前方向的 I/O 没有意义。
    BrokenPipe,
    /// 当前 fd 端点已关闭，不能再进行 I/O 或 poll 等操作。
    Closed,
    /// pipe 容量为零或不满足实现约束。
    InvalidCapacity,
    /// staging buffer 无法分配。
    NoMemory,
}

/// pipe 操作结果。
pub type PipeResult<T> = Result<T, PipeError>;
