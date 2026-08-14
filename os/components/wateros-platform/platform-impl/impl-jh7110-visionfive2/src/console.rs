//! Early console 占位：不输出，调用成功返回（任务 05 接 DW APB UART）。

use api_v0::console::PlatformConsoleResult;

pub fn console_write_a_byte(_byte : u8) -> PlatformConsoleResult<()> { Ok(()) }

pub fn console_write_a_buffer(_bytes : &[u8]) -> PlatformConsoleResult<()> { Ok(()) }

pub fn console_write_raw_buffer(_bytes : &[u8]) -> PlatformConsoleResult<()> { Ok(()) }

pub fn console_flush() -> PlatformConsoleResult<()> { Ok(()) }
