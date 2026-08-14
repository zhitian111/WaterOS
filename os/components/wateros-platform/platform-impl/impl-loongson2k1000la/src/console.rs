//! Early console 占位：不输出，调用成功返回（任务 09 接 NS16550 UART）。

use api_v0::console::PlatformConsoleResult;

pub fn console_write_a_byte(_byte : u8) -> PlatformConsoleResult<()> { Ok(()) }

pub fn console_write_a_buffer(_bytes : &[u8]) -> PlatformConsoleResult<()> { Ok(()) }

pub fn console_write_raw_buffer(_bytes : &[u8]) -> PlatformConsoleResult<()> { Ok(()) }

pub fn console_flush() -> PlatformConsoleResult<()> { Ok(()) }
