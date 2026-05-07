//! 固件控制台：单字节/缓冲写入与 flush 语义（实现常为 SBI 控制台扩展）。

use core::result::Result;

/// 控制台操作失败原因。
#[derive(Debug)]
pub enum FirmwareConsoleError {
    Unavailable,
    Unsupported,
    WriteFailure,
    BufferFailure,
}

/// [`FirmwareConsoleError`] 上的 `Result` 别名。
pub type FirmwareConsoleResult<T> = Result<T, FirmwareConsoleError>;

/// 固件控制台能力：默认实现返回“不支持”，由 `impl-opensbi` 等覆盖。
pub trait FirmwareConsole {
    #[inline]
    fn is_available() -> bool { true }
    #[inline]
    #[allow(unused_variables)]
    fn firmware_console_write_a_byte(byte : u8) -> FirmwareConsoleResult<()> {
        Err(FirmwareConsoleError::Unsupported)
    }
    #[inline]
    fn firmware_console_write_a_buffer(bytes : &[u8]) -> FirmwareConsoleResult<()> {
        if !Self::is_available() {
            Err(FirmwareConsoleError::Unavailable)
        } else {
            for &byte in bytes {
                Self::firmware_console_write_a_byte(byte)?
            }
            Ok(())
        }
    }
    #[inline]
    fn firmware_console_flush() -> FirmwareConsoleResult<()> {
        Err(FirmwareConsoleError::BufferFailure)
    }
}
