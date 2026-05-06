#![no_std]

use api_v0::console::{FirmwareConsole, FirmwareConsoleResult};
use api_v0::reset::{FirmwareReset, FirmwareResetError, FirmwareResetResult};
use api_v0::timer::{FirmwareTimer, FirmwareTimerError, FirmwareTimerResult};
use core::ptr::{read_volatile, write_volatile};

const UART_BASE : usize = 0x1fe0_01e0;
const UART_THR : usize = UART_BASE;
const UART_LSR : usize = UART_BASE + 5;
const UART_LSR_THRE : u8 = 1 << 5;

/// QEMU LoongArch64 virt UART16550 console.
pub struct QemuLoongArch64Uart16550Console;

#[inline]
fn uart_lsr() -> u8 { unsafe { read_volatile(UART_LSR as *const u8) } }

#[inline]
fn uart_write_byte(byte : u8) {
    while (uart_lsr() & UART_LSR_THRE) == 0 {}
    unsafe {
        write_volatile(UART_THR as *mut u8, byte);
    }
}

impl FirmwareConsole for QemuLoongArch64Uart16550Console {
    #[inline]
    fn firmware_console_write_a_byte(byte : u8) -> FirmwareConsoleResult<()> {
        if byte == b'\n' {
            uart_write_byte(b'\r');
        }
        uart_write_byte(byte);
        Ok(())
    }

    #[inline]
    fn firmware_console_flush() -> FirmwareConsoleResult<()> { Ok(()) }
}

pub struct QemuLoongArch64DummyTimer;

impl FirmwareTimer for QemuLoongArch64DummyTimer {
    #[inline]
    fn firmware_set_timer(_time : api_v0::timer::FirmwareTimerDeadline)
                          -> FirmwareTimerResult<()> {
        Err(FirmwareTimerError::Unsupported)
    }
}

pub struct QemuLoongArch64DummyReset;

impl FirmwareReset for QemuLoongArch64DummyReset {
    #[inline]
    fn firmware_reset(_reset_type : api_v0::reset::FirmwareResetType,
                      _reset_reason : api_v0::reset::FirmwareResetReason)
                      -> FirmwareResetResult<()> {
        Err(FirmwareResetError::Unsupported)
    }
}
