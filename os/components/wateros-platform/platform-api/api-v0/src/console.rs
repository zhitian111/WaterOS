//! 平台控制台能力：对内核暴露最小的 early console 输出契约。

use core::result::Result;

/// 平台控制台输出失败时的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformConsoleError {
    /// 当前平台没有可用控制台。
    Unsupported,
    /// 控制台后端暂不可用。
    Unavailable,
    /// 写入失败。
    WriteFailure,
    /// 缓冲/flush 失败。
    BufferFailure,
}

/// [`PlatformConsoleError`] 上的 `Result` 别名。
pub type PlatformConsoleResult<T> = Result<T, PlatformConsoleError>;
