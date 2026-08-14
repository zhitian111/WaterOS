//! 平台外部设备中断门面：按当前 board profile 选择实现。
//!
//! - RISC-V（QEMU virt）：PLIC S-mode（`platform-impl/.../plic.rs`）。
//! - LoongArch（QEMU virt）：EIOINTC → PCH-PIC（`platform-impl/.../external_irq.rs`）。
//!
//! 本模块把平台实现接到通用 IRQ 分发（`irq::domain` / `irq::action`），并暴露给
//! trap 路径与启动路径使用。

use irq::action::IrqReturn;
use irq::types::HwIrq;

#[cfg(feature = "impl-qemu-riscv64-opensbi")]
mod active {
    pub use crate::active_impl::plic::{
        claim, complete, init_current_cpu, register_device_line, set_enabled, ExternalIrqError,
    };
}

#[cfg(feature = "impl-qemu-loongarch64-virt")]
mod active {
    pub use crate::active_impl::external_irq::{
        claim, complete, init_current_cpu, set_enabled, ExternalIrqError,
    };
}

#[cfg(any(feature = "impl-qemu-riscv64-opensbi",
              feature = "impl-qemu-loongarch64-virt"))]
pub use active::ExternalIrqError;

/// 初始化当前 CPU 外部中断上下文并打开架构外部中断使能。
///
/// 必须在平台 MMIO 就绪、全局中断仍关闭时调用，由 BSP/AP 启动路径负责。
pub fn init_current_cpu() -> Result<(), ExternalIrqError> {
    let cpu = crate::arch::cpu::current_cpu_id().raw();
    active::init_current_cpu(cpu)?;
    crate::arch::interrupt::enable_external_interrupt();
    Ok(())
}

/// 使能/关断某条中断线在当前 CPU 的投递。
pub fn set_enabled(irq : u32, enabled : bool) -> Result<(), ExternalIrqError> {
    let cpu = crate::arch::cpu::current_cpu_id().raw();
    active::set_enabled(irq, cpu, enabled)
}

/// claim 当前 CPU 一个 pending IRQ。
pub fn claim() -> Option<u32> {
    let cpu = crate::arch::cpu::current_cpu_id().raw();
    active::claim(cpu)
}

/// complete（EOI）当前 CPU 的 IRQ。
pub fn complete(irq : u32) {
    let cpu = crate::arch::cpu::current_cpu_id().raw();
    active::complete(cpu, irq);
}

/// 注册一条设备中断线并返回 virq（chip 绑定当前 board 的默认投递上下文）。
#[cfg(feature = "impl-qemu-riscv64-opensbi")]
pub fn register_device_line(irq : u32,
                            trigger : crate::irq::types::IrqTrigger)
                            -> crate::irq::IrqResult<crate::irq::types::Virq> {
    let cpu = crate::arch::cpu::current_cpu_id().raw();
    active::register_device_line(cpu, irq, trigger)
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
