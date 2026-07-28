#![no_std]
//! 平台控制台实现：通过 `wateros-platform` 的控制台门面写出字节。
//!
//! **边界**：本模块不选择硬件或固件后端；具体输出路径由 `platform::console`
//! 在当前 board feature 下决定。本模块只把 runtime 的 `fmt::Write` 接到平台门面。

/// 向平台控制台写入单字节；控制台错误按 best-effort 语义忽略。
#[inline]
#[allow(unused)]
pub fn platform_console_write_a_byte(byte : u8) {
    // 日志输出本身不能再次触发 panic，否则会掩盖最初的内核错误。
    let _ = platform::console::console_write_a_byte(byte);
}

/// 将缓冲区原样写入平台控制台（不要求合法 UTF-8）。
#[inline]
#[allow(unused)]
pub fn platform_console_write_a_buffer(bytes : &[u8]) {
    let _ = platform::console::console_write_a_buffer(bytes);
}

/// 在 platform UART 锁内完成整次格式化。
pub fn platform_console_write_fmt(args : core::fmt::Arguments<'_>) {
    let _ = platform::console::console_write_fmt(args);
}

use core::fmt::{self, Write};

/// 基于平台控制台门面的 [`api_v0::Console`] 句柄，无内部状态。
///
/// **契约**：`write_str` 将 UTF-8 切片按字节下发；多字节字符不会被拆分（`str` 保证边界合法）。
#[derive(Default)]
pub struct PlatformConsoleHandle;
impl Write for PlatformConsoleHandle {
    #[inline]
    fn write_str(&mut self, s : &str) -> fmt::Result {
        // 整段下发，避免在 fmt 层引入额外缓冲策略。
        platform_console_write_a_buffer(s.as_bytes());
        Ok(())
    }
}
impl api_v0::Console for PlatformConsoleHandle {}
