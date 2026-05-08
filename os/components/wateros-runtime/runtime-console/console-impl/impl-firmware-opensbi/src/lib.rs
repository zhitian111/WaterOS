#![no_std]
//! OpenSBI 固件控制台实现：通过 `wateros-platform-firmware` 将字节写入 SBI 控制台扩展。
//!
//! **边界**：依赖平台固件 crate 的 `api-v0` 控制台封装，与具体 SBI 版本由该依赖约定；本模块不缓冲、不解释 UTF-8。

/// 向固件控制台写入单字节；错误时 `unwrap`（引导阶段视为致命失败）。
#[inline]
#[allow(unused)]
pub fn firmware_write_a_byte(byte : u8) {
    // 固件返回 Err 表示 SBI 调用失败或环境未就绪；早期内核无恢复策略，直接 panic。
    firmware::console::console_write_a_byte(byte).unwrap();
}

/// 将缓冲区原样写入固件控制台（不要求合法 UTF-8）。
#[inline]
#[allow(unused)]
pub fn firmware_write_a_buffer(bytes : &[u8]) {
    firmware::console::console_write_a_buffer(&bytes).unwrap();
}

use core::fmt::{self, Write};

/// 基于 OpenSBI 固件控制台的 [`api_v0::Console`] 句柄，无内部状态。
///
/// **契约**：`write_str` 将 UTF-8 切片按字节下发；多字节字符不会被拆分（`str` 保证边界合法）。
#[derive(Default)]
pub struct FirmwareConsoleHandle;
impl Write for FirmwareConsoleHandle {
    #[inline]
    fn write_str(&mut self, s : &str) -> fmt::Result {
        // 整段下发，避免在 fmt 层引入额外缓冲策略。
        firmware_write_a_buffer(s.as_bytes());
        Ok(())
    }
}
impl api_v0::Console for FirmwareConsoleHandle {}
