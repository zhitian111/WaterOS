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

/// bottom-half 处理器签名：在可调度上下文执行（回收 virtqueue / 唤醒等待任务）。
///
/// 由 bottom-half 内核任务调用，禁止在其中执行阻塞睡眠；需要睡眠的等待方由
/// 处理器自行唤醒（如 waitqueue）。
pub type BottomHalfFn = fn(Virq, usize);

/// 一条已注册的 IRQ action。
#[derive(Debug, Clone, Copy)]
pub struct IrqAction {
    /// 绑定的虚拟中断号。
    pub virq : Virq,
    /// top-half 处理器。
    pub handler : IrqHandler,
    /// 可选的 bottom-half 处理器；top-half 返回 [`IrqReturn::Handled`] 时由分发
    /// 路径自动调度（见 [`dispatch`]）。
    pub bottom_half : Option<BottomHalfFn>,
    /// 设备上下文 token。
    pub dev_id : usize,
}

/// 注册句柄（action 槽位下标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqHandle(pub usize);

static ACTIONS : Mutex<Vec<Option<IrqAction>>> = Mutex::new(Vec::new());

/// 为已注册的中断线注册一个处理器；同一条 virq 允许多个 action，当前分发只调用
/// 首个匹配（见 [`dispatch`]）。
pub fn request_irq(virq : Virq, handler : IrqHandler, dev_id : usize) -> IrqResult<IrqHandle> {
    request_irq_impl(virq, IrqAction { virq,
                                       handler,
                                       bottom_half : None,
                                       dev_id })
}

/// 注册 top-half + bottom-half 处理器；top-half 返回 [`IrqReturn::Handled`] 时
/// 自动把 bottom-half 调度到可调度上下文（见 [`dispatch`]）。
pub fn request_irq_with_bottom_half(virq : Virq,
                                    handler : IrqHandler,
                                    bottom_half : BottomHalfFn,
                                    dev_id : usize)
                                    -> IrqResult<IrqHandle> {
    request_irq_impl(virq, IrqAction { virq,
                                       handler,
                                       bottom_half:
                                           Some(bottom_half),
                                       dev_id })
}

fn request_irq_impl(virq : Virq, action : IrqAction) -> IrqResult<IrqHandle> {
    if line(virq).is_none() {
        return Err(IrqError::Invalid);
    }
    let mut actions = ACTIONS.lock();
    for (idx, slot) in actions.iter_mut()
                              .enumerate()
    {
        if slot.is_none() {
            *slot = Some(action);
            return Ok(IrqHandle(idx));
        }
    }
    actions.push(Some(action));
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

/// 分发 virq 上注册的处理器（当前取首个匹配 action）。
///
/// 处理器在锁释放后调用，避免 handler 内注册/释放 action 造成自死锁；handler
/// 仍不得调用会长时间持锁的注册表操作。注册表目前是 `spin` 锁，一旦启用真实
/// 设备 IRQ（T06），需要把注册表访问改为中断安全（掩中断/排队），否则同一 CPU
/// 在注册期间被外部中断打断时会自旋死锁。
pub fn dispatch(virq : Virq) -> IrqReturn {
    let target = {
        let actions = ACTIONS.lock();
        actions.iter()
               .flatten()
               .find(|action| action.virq == virq)
               .copied()
    };
    match target {
        Some(action) => {
            let ret = (action.handler)(action.virq, action.dev_id);
            if ret == IrqReturn::Handled &&
               action.bottom_half
                     .is_some()
            {
                crate::bottom_half::schedule(action.virq, action.dev_id);
            }
            ret
        }
        None => IrqReturn::Unhandled,
    }
}

/// 运行 virq 上首个匹配 action 的 bottom-half（由 bottom-half 内核任务调用）。
pub fn run_bottom_half(virq : Virq, dev_id : usize) {
    let target = {
        let actions = ACTIONS.lock();
        actions.iter()
               .flatten()
               .find(|action| action.virq == virq && action.dev_id == dev_id)
               .copied()
    };
    if let Some(action) = target {
        if let Some(bottom_half) = action.bottom_half {
            bottom_half(action.virq, action.dev_id);
        }
    }
}

/// 当前有效 action 数量（诊断用）。
pub fn action_count() -> usize {
    ACTIONS.lock()
           .iter()
           .filter(|slot| slot.is_some())
           .count()
}
