//! QEMU RISC-V `virt` PLIC S-mode 支持：irqchip 与 claim/complete 原语。
//!
//! 寄存器布局来自 QEMU virt 机器 DTB（`plic@c000000`）：
//! - priority / pending / enable / context 四组寄存器；
//! - S-mode context 号 = `hart * 2 + 1`（M 态占偶数，S 态占奇数）；
//! - PLIC MMIO 由 Sv39 内核页表恒等映射（见 `kernel_global::init` 的 PLIC 段）。

use core::ptr::{read_volatile, write_volatile};

use irq::chip::IrqChip;
use irq::types::{HwIrq, IrqAffinity, IrqError, IrqResult, IrqTrigger, Virq};

const PLIC_BASE : usize = 0x0C00_0000;
const PRIORITY_BASE : usize = PLIC_BASE;
const PENDING_BASE : usize = PLIC_BASE + 0x1000;
const ENABLE_BASE : usize = PLIC_BASE + 0x2000;
const ENABLE_STRIDE : usize = 0x80;
const CONTEXT_BASE : usize = PLIC_BASE + 0x20_0000;
const CONTEXT_STRIDE : usize = 0x1000;
/// QEMU virt PLIC 支持的最大中断线（IRQ 0 保留，1..=127 有效）。
const PLIC_MAX_IRQ : u32 = 127;

/// PLIC 操作错误（无字段的单元错误）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalIrqError;

/// S-mode context 号：QEMU virt 中 hart `cpu` 的 S 态上下文为 `2*cpu+1`。
#[inline]
const fn supervisor_context(cpu : usize) -> usize { cpu * 2 + 1 }

// SAFETY 约定：以下 helper 要求 `address` 落在已恒等映射的 PLIC 窗口
// （`QEMU_VIRT_PLIC_PHYS_START..END`）内，由各调用点保证。
#[inline]
unsafe fn read32(address : usize) -> u32 { unsafe { read_volatile(address as *const u32) } }

#[inline]
unsafe fn write32(address : usize, value : u32) {
    unsafe { write_volatile(address as *mut u32, value) }
}

fn validate(irq : u32) -> Result<(), ExternalIrqError> {
    if irq == 0 || irq > PLIC_MAX_IRQ {
        Err(ExternalIrqError)
    } else {
        Ok(())
    }
}

/// 初始化指定 CPU 的 PLIC S-mode 上下文：阈值清零，等待后续逐线使能。
pub fn init_current_cpu(cpu : usize) -> Result<(), ExternalIrqError> {
    let context = supervisor_context(cpu);
    // SAFETY: 内核页表已恒等映射 PLIC 窗口。
    unsafe {
        write32(CONTEXT_BASE + context * CONTEXT_STRIDE,
                0);
    }
    Ok(())
}

/// 使能/关断某条中断线在指定 CPU S-mode 上下文中的投递。
pub fn set_enabled(irq : u32, cpu : usize, enabled : bool) -> Result<(), ExternalIrqError> {
    validate(irq)?;
    let context = supervisor_context(cpu);
    let address = ENABLE_BASE + context * ENABLE_STRIDE + (irq as usize / 32) * 4;
    let bit = 1u32 << (irq % 32);
    // SAFETY: 同上。
    unsafe {
        let old = read32(address);
        write32(address,
                if enabled { old | bit } else { old & !bit });
        if enabled {
            write32(PRIORITY_BASE + irq as usize * 4, 1);
        }
    }
    Ok(())
}

/// 查询指定中断线在指定 CPU 上下文是否已使能。
pub fn is_enabled(irq : u32, cpu : usize) -> Result<bool, ExternalIrqError> {
    validate(irq)?;
    let context = supervisor_context(cpu);
    let address = ENABLE_BASE + context * ENABLE_STRIDE + (irq as usize / 32) * 4;
    // SAFETY: 同上。
    Ok(unsafe { read32(address) } & (1u32 << (irq % 32)) != 0)
}

/// 查询指定中断线是否 pending。
pub fn is_pending(irq : u32) -> Result<bool, ExternalIrqError> {
    validate(irq)?;
    let address = PENDING_BASE + (irq as usize / 32) * 4;
    // SAFETY: 同上。
    Ok(unsafe { read32(address) } & (1u32 << (irq % 32)) != 0)
}

/// claim 指定 CPU S-mode 上下文中的一个 pending IRQ；无 pending 返回 `None`。
pub fn claim(cpu : usize) -> Option<u32> {
    let context = supervisor_context(cpu);
    // SAFETY: 同上。
    let irq = unsafe { read32(CONTEXT_BASE + context * CONTEXT_STRIDE + 4) };
    (irq != 0).then_some(irq)
}

/// complete（EOI）指定 CPU S-mode 上下文中的 IRQ。
pub fn complete(cpu : usize, irq : u32) {
    let context = supervisor_context(cpu);
    // SAFETY: 同上。
    unsafe {
        write32(CONTEXT_BASE + context * CONTEXT_STRIDE + 4,
                irq);
    }
}

/// 绑定到指定 CPU S-mode 上下文的 PLIC irqchip。
///
/// 设备注册时把 hwirq 绑定到目标 CPU 的上下文（当前策略为 BSP/提交者上下文）；
/// claim/complete 由 trap 路径的 [`claim`]/[`complete`] 完成，chip 只负责使能/
/// 关断与 EOI。
#[derive(Clone, Copy)]
pub struct PlicChip(pub usize);

impl IrqChip for PlicChip {
    fn name(&self) -> &'static str { "plic" }

    fn enable(&self, irq : HwIrq) -> IrqResult<()> {
        set_enabled(irq.0, self.0, true).map_err(|_| IrqError::Controller)
    }

    fn disable(&self, irq : HwIrq) -> IrqResult<()> {
        set_enabled(irq.0, self.0, false).map_err(|_| IrqError::Controller)
    }

    fn eoi(&self, irq : HwIrq) -> IrqResult<()> {
        complete(self.0, irq.0);
        Ok(())
    }

    fn set_affinity(&self, _irq : HwIrq, _affinity : IrqAffinity) -> IrqResult<()> {
        Err(IrqError::Unsupported)
    }
}

/// 每 CPU 一个的 PLIC irqchip 实例（设备 IRQ 绑定到注册时的当前 CPU 上下文）。
const fn make_plic_chips<const N : usize>() -> [PlicChip; N] {
    let mut chips = [PlicChip(0); N];
    let mut cpu = 1;
    while cpu < N {
        chips[cpu] = PlicChip(cpu);
        cpu += 1;
    }
    chips
}

static PLIC_CHIPS : [PlicChip; { config::task::MAX_CPUS }] =
    make_plic_chips::<{ config::task::MAX_CPUS }>();

/// 为设备中断线注册 virq 并绑定到 `cpu` 的 PLIC S-mode 上下文；同时使能该线。
pub fn register_device_line(cpu : usize,
                            irq : u32,
                            trigger : IrqTrigger)
                            -> irq::IrqResult<Virq> {
    if cpu >= config::task::MAX_CPUS {
        return Err(irq::IrqError::Invalid);
    }
    set_enabled(irq, cpu, true).map_err(|_| IrqError::Controller)?;
    irq::domain::register_line(HwIrq(irq), &PLIC_CHIPS[cpu], trigger)
}
