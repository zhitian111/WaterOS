//! RISC-V 特权级 **中断使能位** 的薄封装：`sie.STIE` 与 `sstatus.SIE`。
//!
//! 与 **平台 deadline timer**（`mtime` / `set_timer`）解耦；后者在 `platform-impl` 组合。

use api_v0::interrupt::{ArchInterruptState, ArchTimerInterruptControl};
use api_v0::time::ArchTimeResult;
use core::arch::asm;
use riscv::register::{sie, sstatus};

/// RISC-V 架构中断控制实现（与时间计数读取解耦）。
///
/// PLATFORM_BOUNDARY: 所有方法只修改本 hart 的 CSR；timer deadline 由 platform
/// profile 编程，IPI 的 reason 与调度动作不属于本实现。
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
    // `sstatus.SIE` 为 bit1：与 `read_global_interrupt_state` 保存的原始位一致。
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

/// 清除监督态软中断（SSIP），即写 `sip.SSIP = 0`。
///
/// IPI_SYNC: 在取 pending reason 前或后均可，但必须在 trap 返回前调用；否则 SSIP
/// 保持 pending，`sret` 后会立即再次陷入同一个中断。
#[inline]
pub fn clear_soft_interrupt() {
    // SSIP 位于 sip 寄存器的 bit 1；csrc 将目标位清零。
    unsafe {
        asm!("csrc sip, {}", in(reg) 1usize << 1);
    }
}

/// 在当前 hart 打开 SSIE；不会补发此前未处理的 scheduler reason。
#[inline]
pub fn enable_soft_interrupt() {
    unsafe {
        sie::set_ssoft();
    }
}

/// 在当前 hart 关闭 SSIE；仅用于初始化/受控临界阶段，不能作为跨核同步手段。
#[inline]
pub fn disable_soft_interrupt() {
    unsafe {
        sie::clear_ssoft();
    }
}
