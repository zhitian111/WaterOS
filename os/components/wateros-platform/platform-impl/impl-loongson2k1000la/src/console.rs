//! Early NS16550A UART0 输出（2K1000 BSP 基址 0x1FE2_0000，字节 MMIO）。
//!
//! 真机通过 U-Boot 已配好的未缓存 MMIO 段窗口访问（DMW 的 VSEG=8→PSEG=0）：
//! `0x8000_0000_0000_0000 | 0x1FE2_0000`。裸 `0x1FE2_0000` 会落到 DRAM 窗口，
//! 写不出串口。

use api_v0::console::{PlatformConsoleError, PlatformConsoleResult};
use core::ptr::{read_volatile, write_volatile};

const UART_BASE : usize = 0x8000_0000_1FE2_0000;
const UART_LSR : usize = UART_BASE + 5;
const LSR_THRE : u8 = 1 << 5;
const LSR_TEMT : u8 = 1 << 6;
const SPIN_MAX : usize = 1_000_000;

fn write_raw(byte : u8) -> PlatformConsoleResult<()> {
    for _ in 0..SPIN_MAX {
        if unsafe { read_volatile(UART_LSR as *const u8) } & LSR_THRE != 0 {
            unsafe { write_volatile(UART_BASE as *mut u8, byte) };
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
        if unsafe { read_volatile(UART_LSR as *const u8) } & LSR_TEMT != 0 {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(PlatformConsoleError::WriteFailure)
}
