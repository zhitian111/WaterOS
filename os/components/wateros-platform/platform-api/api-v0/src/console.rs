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

/// 平台控制台能力；实现可以是 SBI console、MMIO UART 或其它 board 后端。
pub trait PlatformConsole {
    #[inline]
    fn platform_console_write_a_byte(_byte : u8) -> PlatformConsoleResult<()> {
        Err(PlatformConsoleError::Unsupported)
    }

    #[inline]
    fn platform_console_write_a_buffer(bytes : &[u8]) -> PlatformConsoleResult<()> {
        if !bytes.is_empty() && !Self::is_available() {
            return Err(PlatformConsoleError::Unavailable);
        }
        for &byte in bytes {
            Self::platform_console_write_a_byte(byte)?;
        }
        Ok(())
    }

    #[inline]
    fn platform_console_flush() -> PlatformConsoleResult<()> {
        Err(PlatformConsoleError::BufferFailure)
    }

    #[inline]
    fn is_available() -> bool { true }
}
