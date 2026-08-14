//! 平台外部设备中断门面（QEMU RISC-V virt PLIC）。
//!
//! 本模块把 `platform-impl` 的 PLIC 原语接到通用 IRQ 分发（`irq::domain` /
//! `irq::action`），并暴露给 trap 路径与启动路径使用。

use irq::action::IrqReturn;
use irq::types::HwIrq;

pub use crate::active_impl::plic::ExternalIrqError;

/// 初始化当前 CPU 的 PLIC S-mode 上下文并打开 `sie.SEIE`。
///
/// 必须在 Sv39 内核页表（含 PLIC 恒等映射）就绪、全局中断仍关闭时调用，
/// 由 BSP/AP 启动路径负责。
pub fn init_current_cpu() -> Result<(), ExternalIrqError> {
    let cpu = crate::arch::cpu::current_cpu_id().raw();
    crate::active_impl::plic::init_current_cpu(cpu)?;
    crate::arch::interrupt::enable_external_interrupt();
    Ok(())
}

/// 使能/关断某条中断线在当前 CPU 的 PLIC 上下文投递。
pub fn set_enabled(irq : u32, enabled : bool) -> Result<(), ExternalIrqError> {
    let cpu = crate::arch::cpu::current_cpu_id().raw();
    crate::active_impl::plic::set_enabled(irq, cpu, enabled)
}

/// claim 当前 CPU PLIC 上下文中的一个 pending IRQ。
pub fn claim() -> Option<u32> {
    let cpu = crate::arch::cpu::current_cpu_id().raw();
    crate::active_impl::plic::claim(cpu)
}

/// complete（EOI）当前 CPU PLIC 上下文中的 IRQ。
pub fn complete(irq : u32) {
    let cpu = crate::arch::cpu::current_cpu_id().raw();
    crate::active_impl::plic::complete(cpu, irq);
}

/// 分发当前 CPU 全部 pending 外部中断：claim → 查找/调用 action → complete。
///
/// 在 trap 的 `SupervisiorExternel` 分支调用；无对应 action 的中断记录 warn 后
/// 照常 complete，避免未使能的杂散中断卡死 trap 返回。
pub fn dispatch_external() {
    let cpu = crate::arch::cpu::current_cpu_id().raw();
    while let Some(hwirq) = claim() {
        let handled = match irq::domain::line_by_hwirq(HwIrq(hwirq)) {
            Some(line) => irq::action::dispatch(line.virq),
            None => IrqReturn::Unhandled,
        };
        if handled == IrqReturn::Unhandled {
            log::warn!("[irq] unhandled external IRQ {} on cpu {}",
                       hwirq,
                       cpu);
        }
        complete(hwirq);
    }
}
