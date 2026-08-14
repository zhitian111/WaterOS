//! 中断控制器（irqchip）抽象：对应 Linux genirq 的 `struct irq_chip`。
//!
//! 每个平台 / 板级控制器（RISC-V PLIC、LoongArch EIOINTC、真机 LIOINTC 等）实现
//! 本 trait；trap 分发与 irq domain 只通过它操作中断线，不感知具体寄存器。
//!
//! ## 语义约定
//! - `enable/disable` 控制中断线的全局可用性；
//! - `mask/unmask` 用于运行期快速开关（默认退化为 enable/disable）；
//! - `ack/eoi` 对应流控阶段：电平型控制器（如 PLIC）的 `eoi` 即 complete，
//!   claim 在分发入口完成；无独立 ack/eoi 语义的控制器可实现为空操作。

use crate::types::{HwIrq, IrqAffinity, IrqError, IrqResult};

pub trait IrqChip: Send + Sync {
    /// 控制器名称，用于日志与诊断。
    fn name(&self) -> &'static str;

    /// 使能中断线 `irq`。
    fn enable(&self, irq : HwIrq) -> IrqResult<()>;

    /// 关断中断线 `irq`。
    fn disable(&self, irq : HwIrq) -> IrqResult<()>;

    /// 掩码（快速关闭投递，不改变 enable 状态）。默认退化为 [`Self::disable`]。
    fn mask(&self, irq : HwIrq) -> IrqResult<()> { self.disable(irq) }

    /// 去掩码。默认退化为 [`Self::enable`]。
    fn unmask(&self, irq : HwIrq) -> IrqResult<()> { self.enable(irq) }

    /// 中断确认。无独立 ack 语义的控制器默认空操作。
    fn ack(&self, _irq : HwIrq) -> IrqResult<()> { Ok(()) }

    /// 中断结束（EOI）。PLIC 等电平型控制器应在 complete 阶段调用。
    fn eoi(&self, _irq : HwIrq) -> IrqResult<()> { Ok(()) }

    /// 设置中断亲和性；不支持的控制器返回 [`IrqError::Unsupported`]。
    fn set_affinity(&self, _irq : HwIrq, _affinity : IrqAffinity) -> IrqResult<()> {
        Err(IrqError::Unsupported)
    }
}
