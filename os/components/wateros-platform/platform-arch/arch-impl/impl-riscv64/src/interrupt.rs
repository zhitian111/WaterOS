<<<<<<< HEAD
//! RISC-V 特权级 **中断使能位** 的薄封装：`sie.STIE` 与 `sstatus.SIE`。
//!
//! 与 **固件定时器**（`mtime` / `set_timer`）解耦；后者在 `platform-firmware` 组合。

use api_v0::interrupt::ArchTimerInterruptControl;
=======
use api_v0::interrupt::{ArchInterruptState, ArchTimerInterruptControl};
>>>>>>> github/main
use api_v0::time::ArchTimeResult;
use core::arch::asm;
use riscv::register::{sie, sstatus};

/// RISC-V 架构中断控制实现（与时间计数读取解耦）。
pub struct Riscv64ArchInterrupt;

impl ArchTimerInterruptControl for Riscv64ArchInterrupt {
    #[inline]
    fn enable_timer_interrupt() -> ArchTimeResult<()> {
        unsafe {
            sie::set_stimer();
        }
        Ok(())
    }

    #[inline]
    fn disable_timer_interrupt() -> ArchTimeResult<()> {
        unsafe {
            sie::clear_stimer();
        }
        Ok(())
    }

    #[inline]
    fn enable_global_interrupt() -> ArchTimeResult<()> {
        unsafe {
            sstatus::set_sie();
        }
        Ok(())
    }

    #[inline]
    fn disable_global_interrupt() -> ArchTimeResult<()> {
        unsafe {
            sstatus::clear_sie();
        }
        Ok(())
    }

    #[inline]
    fn read_global_interrupt_state() -> ArchTimeResult<ArchInterruptState> {
        let value = sstatus::read().bits();
        Ok(ArchInterruptState(value))
    }

    #[inline]
    fn restore_global_interrupt_state(state: ArchInterruptState) -> ArchTimeResult<()> {
        if (state.0 & (1 << 1)) != 0 {
            Self::enable_global_interrupt()
        } else {
            Self::disable_global_interrupt()
        }
    }

    #[inline]
    fn wait_for_interrupt() {
        unsafe {
            asm!("wfi");
        }
    }
}
