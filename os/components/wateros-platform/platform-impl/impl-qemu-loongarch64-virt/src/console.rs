//! QEMU LoongArch64 `virt` 早期 UART16550 控制台。
//!
//! 这是 board 层的 early console：用于 runtime logging 初始化前后的最小输出。
//! 完整串口设备对象位于 `wateros-driver` 的 LoongArch QEMU 实现中。

use core::ptr::{read_volatile, write_volatile};
use api_v0::console::PlatformConsoleResult;

/// QEMU LoongArch64 `virt` UART16550 默认 MMIO 物理基址。
const UART_BASE: usize = 0x1FE0_01E0;
const UART_THR: usize = UART_BASE;
const UART_LSR: usize = UART_BASE + 5;
const UART_LSR_THRE: u8 = 1 << 5;

#[inline]
fn uart_lsr() -> u8 {
    unsafe { read_volatile(UART_LSR as *const u8) }
}

#[inline]
fn uart_write_byte_raw(byte: u8) {
    while (uart_lsr() & UART_LSR_THRE) == 0 {
        core::hint::spin_loop();
    }
    unsafe {
        write_volatile(UART_THR as *mut u8, byte);
    }
}

/// 向早期 UART 控制台写入单字节。
#[inline]
pub fn console_write_a_byte(byte: u8) -> PlatformConsoleResult<()> {
    if byte == b'\n' {
        uart_write_byte_raw(b'\r');
    }
    uart_write_byte_raw(byte);
    Ok(())
}

/// 将缓冲区原样写入早期 UART 控制台。
#[inline]
pub fn console_write_a_buffer(bytes: &[u8]) -> PlatformConsoleResult<()> {
    for &byte in bytes {
        console_write_a_byte(byte)?;
    }
    Ok(())
}

/// UART 轮询输出没有独立 flush 通道。
#[inline]
pub fn console_flush() -> PlatformConsoleResult<()> {
    Ok(())
}
