//! 不可用的占位控制台后端。

use api_v0::console::{PlatformConsoleError, PlatformConsoleResult};

#[inline]
pub fn console_flush() -> PlatformConsoleResult<()> { Err(PlatformConsoleError::Unsupported) }

#[inline]
pub fn console_write_a_byte(_ : u8) -> PlatformConsoleResult<()> {
    Err(PlatformConsoleError::Unsupported)
}

#[inline]
pub fn console_write_a_buffer(_ : &[u8]) -> PlatformConsoleResult<()> {
    Err(PlatformConsoleError::Unsupported)
}

#[inline]
pub fn console_write_raw_buffer(_ : &[u8]) -> PlatformConsoleResult<()> {
    Err(PlatformConsoleError::Unsupported)
}
