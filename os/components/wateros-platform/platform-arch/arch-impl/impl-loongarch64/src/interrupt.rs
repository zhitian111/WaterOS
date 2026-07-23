//! LoongArch64 **中断开关**：`CRMD.IE` 为全局中断，`ECFG` 中定时器使能位与手册一致；
//! **不**编程 StableCounter deadline（见 `platform::timer`）。

use api_v0::interrupt::{ArchInterruptState, ArchTimerInterruptControl};
use api_v0::time::ArchTimeResult;
use core::arch::asm;

/// 当前模式配置 CSR：本文件仅用 `IE` 位反映全局中断开关快照。
const CSR_CRMD: usize = 0x0;
/// 异常配置 CSR：`VS=11` 位使能定时器类中断（与 `enable_timer_interrupt` 对应）。
const CSR_ECFG: usize = 0x4;
/// `CRMD.IE`：全局中断使能。
const CRMD_IE: usize = 1 << 2;
/// `ECFG` 定时器中断使能掩码（与 `TIMER_INTERRUPT_PENDING` 路径配套使用）。
const ECFG_TIMER_INTERRUPT_ENABLE: usize = 1 << 11;
/// `ECFG.IS.IPI`：LoongArch IPI 中断使能位。
const ECFG_IPI_INTERRUPT_ENABLE: usize = 1 << 12;

/// LoongArch64 架构中断控制实现。
pub struct LoongArch64ArchInterrupt;

#[inline]
fn read_csr<const CSR: usize>() -> usize {
    let value: usize;
    unsafe {
        asm!("csrrd {0}, {1}", out(reg) value, const CSR);
    }
    value
}

#[inline]
fn write_csr<const CSR: usize>(value: usize) {
    let old = value;
    unsafe {
        asm!("csrwr {0}, {1}", inout(reg) old => _, const CSR);
    }
}

impl ArchTimerInterruptControl for LoongArch64ArchInterrupt {
    #[inline]
    fn enable_timer_interrupt() -> ArchTimeResult<()> {
        write_csr::<CSR_ECFG>(read_csr::<CSR_ECFG>() | ECFG_TIMER_INTERRUPT_ENABLE);
        Ok(())
    }

    #[inline]
    fn disable_timer_interrupt() -> ArchTimeResult<()> {
        write_csr::<CSR_ECFG>(read_csr::<CSR_ECFG>() & !ECFG_TIMER_INTERRUPT_ENABLE);
        Ok(())
    }

    #[inline]
    fn enable_global_interrupt() -> ArchTimeResult<()> {
        write_csr::<CSR_CRMD>(read_csr::<CSR_CRMD>() | CRMD_IE);
        Ok(())
    }

    #[inline]
    fn disable_global_interrupt() -> ArchTimeResult<()> {
        write_csr::<CSR_CRMD>(read_csr::<CSR_CRMD>() & !CRMD_IE);
        Ok(())
    }

    #[inline]
    fn read_global_interrupt_state() -> ArchTimeResult<ArchInterruptState> {
        Ok(ArchInterruptState(
            read_csr::<CSR_CRMD>(),
        ))
    }

    #[inline]
    fn restore_global_interrupt_state(state: ArchInterruptState) -> ArchTimeResult<()> {
        if (state.0 & CRMD_IE) != 0 {
            Self::enable_global_interrupt()
        } else {
            Self::disable_global_interrupt()
        }
    }

    #[inline]
    fn wait_for_interrupt() {
        unsafe {
            asm!("idle 0");
        }
    }
}

/// LoongArch SMP IPI support is not enabled yet.  Keep the architecture-neutral
/// interrupt facade available so shared scheduler code can still build.
#[inline]
pub fn clear_soft_interrupt() {}
#[inline]
pub fn enable_soft_interrupt() {
    write_csr::<CSR_ECFG>(read_csr::<CSR_ECFG>() | ECFG_IPI_INTERRUPT_ENABLE);
}
#[inline]
pub fn disable_soft_interrupt() {
    write_csr::<CSR_ECFG>(read_csr::<CSR_ECFG>() & !ECFG_IPI_INTERRUPT_ENABLE);
}
