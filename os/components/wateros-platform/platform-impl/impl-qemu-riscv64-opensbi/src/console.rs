//! 本模块代码由AI完成
//! OpenSBI 控制台后端。

use api_v0::console::{PlatformConsoleError, PlatformConsoleResult};
#[allow(unused)]
use sbi::{console_write, console_write_byte};

/// 向 OpenSBI console 写入单字节。
#[inline]
pub fn console_write_a_byte(byte: u8) -> PlatformConsoleResult<()> {
    if console_write_byte(byte).is_ok() {
        Ok(())
    } else {
        Err(PlatformConsoleError::WriteFailure)
    }
}

/// 将缓冲区逐字节写入 OpenSBI console。
#[inline]
pub fn console_write_a_buffer(bytes: &[u8]) -> PlatformConsoleResult<()> {
    for &byte in bytes {
        console_write_a_byte(byte)?;
    }
    Ok(())
}

/// SBI debug console 没有额外 flush 语义。
#[inline]
pub fn console_flush() -> PlatformConsoleResult<()> {
    Ok(())
}
