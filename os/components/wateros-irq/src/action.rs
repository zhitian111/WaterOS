//! IRQ action 注册表：把中断线与处理器绑定，对应 Linux `struct irqaction`。
//!
//! T01 只提供注册 / 释放 / 查询；实际分发在 trap 路径（T02）接入，ISR 只做极简
//! top-half，heavy 工作在 bottom-half（T04）完成。

use alloc::vec::Vec;
use spin::Mutex;

use crate::domain::line;
use crate::types::{IrqError, IrqResult, Virq};

/// ISR 返回值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqReturn {
    /// 本 handler 已处理该中断。
    Handled,
    /// 本 handler 未处理（可能无对应设备活动）。
    Unhandled,
}

/// 中断处理器签名：`fn(virq, dev_id)`。
///
/// `dev_id` 是注册时传入的设备上下文 token（如设备指针 / 句柄），由分发路径在
/// 调用时回传，保证 `fn` 指针无需闭包捕获即可定位设备状态。
pub type IrqHandler = fn(Virq, usize) -> IrqReturn;

/// 一条已注册的 IRQ action。
#[derive(Debug, Clone, Copy)]
pub struct IrqAction {
    /// 绑定的虚拟中断号。
    pub virq : Virq,
    /// top-half 处理器。
    pub handler : IrqHandler,
    /// 设备上下文 token。
    pub dev_id : usize,
}

/// 注册句柄（action 槽位下标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqHandle(pub usize);

static ACTIONS : Mutex<Vec<Option<IrqAction>>> = Mutex::new(Vec::new());

/// 为已注册的中断线注册一个处理器；同一条 virq 允许多个 action（共享语义，
/// 分发时逐个尝试直到 [`IrqReturn::Handled`]）。
pub fn request_irq(virq : Virq, handler : IrqHandler, dev_id : usize) -> IrqResult<IrqHandle> {
    if line(virq).is_none() {
        return Err(IrqError::Invalid);
    }
    let mut actions = ACTIONS.lock();
    for (idx, slot) in actions.iter_mut()
                              .enumerate()
    {
        if slot.is_none() {
            *slot = Some(IrqAction { virq,
                                     handler,
                                     dev_id });
            return Ok(IrqHandle(idx));
        }
    }
    actions.push(Some(IrqAction { virq,
                                  handler,
                                  dev_id }));
    Ok(IrqHandle(actions.len() - 1))
}

/// 释放注册的处理器；句柄不存在或已释放返回 `false`。
pub fn free_irq(handle : IrqHandle) -> bool {
    let mut actions = ACTIONS.lock();
    match actions.get_mut(handle.0) {
        Some(slot) if slot.is_some() => {
            *slot = None;
            true
        }
        _ => false,
    }
}

/// 查询句柄对应的 action。
pub fn action(handle : IrqHandle) -> Option<IrqAction> {
    ACTIONS.lock()
           .get(handle.0)
           .copied()
           .flatten()
}

/// 当前有效 action 数量（诊断用）。
pub fn action_count() -> usize {
    ACTIONS.lock()
           .iter()
           .filter(|slot| slot.is_some())
           .count()
}
