//! irq domain：把 `(控制器, hwirq)` 映射为统一 virq，并保存 chip 与触发方式。
//!
//! 对应 Linux `struct irq_domain`。本模块只做运行期注册与查询；DTB 的静态解析
//! 在 driver-impl（`parse_irq_specs`），设备注册阶段把解析结果交给本模块登记，
//! 驱动持有返回的 [`Virq`] 即与具体控制器解耦。

use alloc::vec::Vec;
use spin::Mutex;

use crate::chip::IrqChip;
use crate::types::{HwIrq, IrqError, IrqResult, IrqTrigger, Virq};

/// 一条已注册的中断线（运行期描述）。
#[derive(Clone, Copy)]
pub struct IrqLine {
    /// 分配得到的虚拟中断号。
    pub virq : Virq,
    /// 控制器本地硬件中断号。
    pub hwirq : HwIrq,
    /// 所属中断控制器。
    pub chip : &'static dyn IrqChip,
    /// 触发方式。
    pub trigger : IrqTrigger,
}

static LINES : Mutex<Vec<IrqLine>> = Mutex::new(Vec::new());

/// 注册一条中断线并分配 virq。
///
/// virq 即表内下标，单调递增、稳定；T01 不提供释放/复用，后续需要时再引入
/// 槽位回收。
pub fn register_line(hwirq : HwIrq,
                     chip : &'static dyn IrqChip,
                     trigger : IrqTrigger)
                     -> IrqResult<Virq> {
    let mut lines = LINES.lock();
    let virq = Virq(u32::try_from(lines.len()).map_err(|_| IrqError::Unsupported)?);
    lines.push(IrqLine { virq,
                         hwirq,
                         chip,
                         trigger });
    Ok(virq)
}

/// 查询 virq 对应的中断线描述。
pub fn line(virq : Virq) -> Option<IrqLine> {
    LINES.lock()
         .get(virq.0 as usize)
         .copied()
}

/// 按硬件中断号反查中断线（分发路径用；同一 hwirq 重复注册时返回首个）。
pub fn line_by_hwirq(hwirq : HwIrq) -> Option<IrqLine> {
    LINES.lock()
         .iter()
         .copied()
         .find(|entry| entry.hwirq == hwirq)
}

/// 已注册中断线数量（诊断与自检用）。
pub fn line_count() -> usize { LINES.lock().len() }

/// 清除全部注册项（仅单元测试使用；内核运行期不应调用）。
#[cfg(test)]
pub fn reset_for_test() { LINES.lock().clear(); }
