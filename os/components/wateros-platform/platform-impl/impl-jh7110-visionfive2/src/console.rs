//! Early DW APB UART0 输出。固件必须已初始化 UART0 线参数。

use api_v0::console::{PlatformConsoleError, PlatformConsoleResult};
use core::ptr::{read_volatile, write_volatile};

const UART_BASE : usize = 0x1000_0000;
const UART_THR : usize = UART_BASE;
const UART_LSR : usize = UART_BASE + 0x14;
const LSR_THRE : u32 = 1 << 5;
const LSR_TEMT : u32 = 1 << 6;
const SPIN_MAX : usize = 1_000_000;

fn write_raw(byte : u8) -> PlatformConsoleResult<()> {
    for _ in 0..SPIN_MAX {
        if unsafe { read_volatile(UART_LSR as *const u32) } & LSR_THRE != 0 {
            unsafe { write_volatile(UART_THR as *mut u32, byte as u32) };
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(PlatformConsoleError::WriteFailure)
}

pub fn console_write_a_byte(byte : u8) -> PlatformConsoleResult<()> {
    if byte == b'\n' {
        write_raw(b'\r')?;
    }
    write_raw(byte)
}

pub fn console_write_a_buffer(bytes : &[u8]) -> PlatformConsoleResult<()> {
    for &byte in bytes {
        console_write_a_byte(byte)?;
    }
    Ok(())
}

pub fn console_write_raw_buffer(bytes : &[u8]) -> PlatformConsoleResult<()> {
    for &byte in bytes {
        write_raw(byte)?;
    }
    Ok(())
}

pub fn console_flush() -> PlatformConsoleResult<()> {
    for _ in 0..SPIN_MAX {
        if unsafe { read_volatile(UART_LSR as *const u32) } & LSR_TEMT != 0 {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(PlatformConsoleError::WriteFailure)
}
