#![no_std]
//! OpenSBI 固件控制台实现：通过 `wateros-platform-firmware` 将字节写入 SBI 控制台扩展。

/// 向固件控制台写入单字节；错误时 `unwrap`（引导阶段视为致命失败）。
#[inline]
#[allow(unused)]
pub fn firmware_write_a_byte(byte : u8) { firmware::console::console_write_a_byte(byte).unwrap(); }

/// 将缓冲区原样写入固件控制台（不要求合法 UTF-8）。
#[inline]
#[allow(unused)]
pub fn firmware_write_a_buffer(bytes : &[u8]) {
    firmware::console::console_write_a_buffer(&bytes).unwrap();
}

use core::fmt::{self, Write};

/// 基于 OpenSBI 固件控制台的 [`api_v0::Console`] 句柄，无内部状态。
#[derive(Default)]
pub struct FirmwareConsoleHandle;
impl Write for FirmwareConsoleHandle {
    #[inline]
    fn write_str(&mut self, s : &str) -> fmt::Result {
        firmware_write_a_buffer(s.as_bytes());
        Ok(())
    }
}
impl api_v0::Console for FirmwareConsoleHandle {}
