#![no_std]

//! QEMU LoongArch64 **早期 I/O**：UART16550 MMIO 控制台 + **CSR 定时器**编程（与 arch
//! `StableCounter` tick 同一刻度组合使用）。
//!
//! **假设**：`UART_BASE` 与 QEMU virt 设备树一致；CSR 编号与 LoongArch 手册及当前内核
//! trap 路径中 `TCFG`/`TICLR` 使用保持一致。

use api_v0::console::{FirmwareConsole, FirmwareConsoleResult};
use api_v0::reset::{FirmwareReset, FirmwareResetError, FirmwareResetResult};
use api_v0::timer::{FirmwareTimer, FirmwareTimerError, FirmwareTimerResult};
use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

/// QEMU virt 板上 UART16550 物理基址（与设备树 `reg` 一致；换板须同步修改）。
const UART_BASE : usize = 0x1FE0_01E0;
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

/// 定时器配置 CSR：写入 `(delta << 2) | ENABLE` 形式与硬件解码约定一致。
const CSR_TCFG : usize = 0x41;
/// 定时器中断清除 CSR：与 `arch-impl-loongarch64` trap 中清挂起位一致。
const CSR_TICLR : usize = 0x44;
const TCFG_ENABLE : usize = 1 << 0;
const TICLR_CLEAR_TIMER : usize = 1 << 0;

/// 用 **CSR 定时器**近似 SBI `set_timer`：deadline 与当前 `rdtime.d` 的差写入 `TCFG`。
pub struct QemuLoongArch64Timer;

#[inline]
fn read_stable_counter() -> u64 {
    let tick : u64;
    let _counter_id : usize;
    unsafe {
        asm!("rdtime.d {0}, {1}", out(reg) tick, out(reg) _counter_id);
    }
    tick
}

#[inline]
fn write_csr<const CSR: usize>(value : usize) {
    let old = value;
    unsafe {
        asm!("csrwr {0}, {1}", inout(reg) old => _, const CSR);
    }
}

impl FirmwareTimer for QemuLoongArch64Timer {
    #[inline]
    fn firmware_set_timer(_time : api_v0::timer::FirmwareTimerDeadline) -> FirmwareTimerResult<()> {
        let now = read_stable_counter();
        let delta = _time.0
                         .saturating_sub(now)
                         .max(1);
        let delta = usize::try_from(delta).map_err(|_| FirmwareTimerError::InvalidDeadline)?;
        if delta > (usize::MAX >> 2) {
            return Err(FirmwareTimerError::InvalidDeadline);
        }
        write_csr::<CSR_TICLR>(TICLR_CLEAR_TIMER);
        write_csr::<CSR_TCFG>((delta << 2) | TCFG_ENABLE);
        Ok(())
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
