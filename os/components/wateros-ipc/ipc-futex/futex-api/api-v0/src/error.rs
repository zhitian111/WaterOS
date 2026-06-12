//! Futex 错误与结果类型。

/// Futex 操作错误（与 Linux errno 对齐的 IPC 层视图，syscall 层负责最终映射）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FutexError {
    /// 用户地址上的值与期望不符（`EAGAIN`）。
    Again,
    /// 用户内存访问失败（`EFAULT`）。
    Fault,
    /// 参数非法（`EINVAL`）。
    Invalid,
    /// 操作码或变体尚未支持（`ENOSYS`）。
    Nosys,
    /// 带超时等待超时（`ETIMEDOUT`）。
    TimedOut,
    /// 等待被信号中断（`EINTR`）。
    Interrupted,
}

/// Futex 操作结果。
pub type FutexResult<T> = Result<T, FutexError>;
