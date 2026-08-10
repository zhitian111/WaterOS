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
/// EIOINTC is wired to CPU interrupt input 3 on QEMU virt.  Hardware
/// interrupt inputs occupy ESTAT/ECFG bits 2..9, hence input 3 is bit 5.
const ECFG_EXTERNAL_INTERRUPT_ENABLE: usize = 1 << 5;
/// LoongArch IOCSR IPI pending/clear 寄存器。
const IOCSR_IPI_STATUS: usize = 0x1000;
const IOCSR_IPI_CLEAR: usize = 0x100C;

/// LoongArch64 架构中断控制实现。
///
/// PLATFORM_BOUNDARY: 这些操作只影响当前 CPU 的 CRMD/ECFG/IOCSR 状态；IPI transport
/// 的目标选择和 mailbox 参数仍由 QEMU platform profile 维护。
pub struct LoongArch64ArchInterrupt;

impl LoongArch64ArchInterrupt {
    /// Enable the platform's external device interrupt line.
    #[inline]
    pub fn enable_external_interrupt() {
        write_csr::<CSR_ECFG>(read_csr::<CSR_ECFG>() | ECFG_EXTERNAL_INTERRUPT_ENABLE);
    }

    /// Disable the platform's external device interrupt line.
    #[inline]
    pub fn disable_external_interrupt() {
        write_csr::<CSR_ECFG>(read_csr::<CSR_ECFG>() & !ECFG_EXTERNAL_INTERRUPT_ENABLE);
    }
}

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

#[inline]
fn iocsr_read32(address: usize) -> u32 {
    let value: u32;
    unsafe {
        asm!("iocsrrd.w {value}, {address}", value = out(reg) value,
             address = in(reg) address, options(nostack));
    }
    value
}

#[inline]
fn iocsr_write32(value: u32, address: usize) {
    unsafe {
        asm!("iocsrwr.w {value}, {address}", value = in(reg) value,
             address = in(reg) address, options(nostack));
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

/// 清除当前 CPU 的 LoongArch IOCSR IPI pending 位。
///
/// IPI_SYNC: 这与发送 IPI 的 transport 无关；必须在本核的中断入口内完成，否则
/// `idle`/trap 返回后仍会观察到同一个 pending 中断。
#[inline]
pub fn clear_soft_interrupt() {
    let pending = iocsr_read32(IOCSR_IPI_STATUS);
    if pending != 0 {
        iocsr_write32(pending, IOCSR_IPI_CLEAR);
    }
}
#[inline]
/// 打开当前 CPU 的 IPI 中断使能位；不清除已有 pending 状态。
pub fn enable_soft_interrupt() {
    write_csr::<CSR_ECFG>(read_csr::<CSR_ECFG>() | ECFG_IPI_INTERRUPT_ENABLE);
}
#[inline]
/// 关闭当前 CPU 的 IPI 中断使能位；调用者仍需在适当时机清除 pending。
pub fn disable_soft_interrupt() {
    write_csr::<CSR_ECFG>(read_csr::<CSR_ECFG>() & !ECFG_IPI_INTERRUPT_ENABLE);
}
