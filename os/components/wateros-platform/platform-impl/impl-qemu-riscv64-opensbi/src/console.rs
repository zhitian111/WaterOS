//! 本模块代码由AI完成
//! QEMU virt NS16550 UART0 控制台后端。

use api_v0::console::{PlatformConsoleError, PlatformConsoleResult};
use core::ptr::{read_volatile, write_volatile};

const UART_BASE : usize = config::mm::QEMU_VIRT_MMIO_PHYS_START;
const UART_THR : usize = UART_BASE;
const UART_LSR : usize = UART_BASE + 5;
const UART_LSR_THRE : u8 = 1 << 5;
const UART_LSR_TEMT : u8 = 1 << 6;
const SPIN_TX_MAX : usize = 1_000_000;

fn write_raw(byte : u8) -> PlatformConsoleResult<()> {
    for _ in 0..SPIN_TX_MAX {
        let ready = unsafe { read_volatile(UART_LSR as *const u8) };
        if ready & UART_LSR_THRE != 0 {
            unsafe { write_volatile(UART_THR as *mut u8, byte) };
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(PlatformConsoleError::WriteFailure)
}

/// 向 UART0 写入单字节。
#[inline]
pub fn console_write_a_byte(byte : u8) -> PlatformConsoleResult<()> {
    if byte == b'\n' {
        write_raw(b'\r')?;
    }
    write_raw(byte)
}

/// 将缓冲区逐字节写入 UART0。
#[inline]
pub fn console_write_a_buffer(bytes : &[u8]) -> PlatformConsoleResult<()> {
    for &byte in bytes {
        console_write_a_byte(byte)?;
    }
    Ok(())
}

/// 等待发送保持寄存器和移位寄存器均为空。
#[inline]
pub fn console_flush() -> PlatformConsoleResult<()> {
    for _ in 0..SPIN_TX_MAX {
        let status = unsafe { read_volatile(UART_LSR as *const u8) };
        if status & UART_LSR_TEMT != 0 {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(PlatformConsoleError::WriteFailure)
}
