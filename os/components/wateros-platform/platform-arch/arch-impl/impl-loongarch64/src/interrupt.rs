use api_v0::interrupt::{ArchInterruptState, ArchTimerInterruptControl};
use api_v0::time::ArchTimeResult;
use core::arch::asm;

const CSR_CRMD: usize = 0x0;
const CSR_ECFG: usize = 0x4;
const CRMD_IE: usize = 1 << 2;
const ECFG_TIMER_INTERRUPT_ENABLE: usize = 1 << 11;

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
