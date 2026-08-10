//! 固定容量的设备 IRQ handler registry。
//!
//! 平台控制器负责 claim/complete；本模块只负责把一个硬件 IRQ 号分发到
//! 已在启动期注册的设备 handler。分发路径禁止分配和阻塞。

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

/// 平台统一使用的设备 IRQ 编号。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrqNumber(pub u32);

/// handler 是否认领了该 IRQ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrqHandled {
    No,
    Yes,
}

/// 硬件中断上下文中执行的回调。
pub type IrqHandlerFn = unsafe fn(IrqNumber, usize) -> IrqHandled;

#[derive(Clone, Copy)]
struct Entry {
    irq: IrqNumber,
    handler: IrqHandlerFn,
    context: usize,
}

const MAX_IRQ_HANDLERS: usize = 32;
static REGISTRY: Mutex<[Option<Entry>; MAX_IRQ_HANDLERS]> = Mutex::new([None; MAX_IRQ_HANDLERS]);
static FROZEN: AtomicBool = AtomicBool::new(false);
static HANDLED: AtomicU64 = AtomicU64::new(0);
static SPURIOUS: AtomicU64 = AtomicU64::new(0);

/// 注册一个 handler；只能在设备 IRQ 开启前调用。
///
/// # Safety
///
/// `context` 必须在 handler 注销或系统停止接收 IRQ 前一直有效；handler 不得
/// 分配、阻塞、获取可能被硬中断打断的不可重入锁或调用文件系统。
pub unsafe fn register_handler(
    irq: IrqNumber,
    handler: IrqHandlerFn,
    context: usize,
) -> bool {
    if FROZEN.load(Ordering::Acquire) {
        return false;
    }
    let mut registry = REGISTRY.lock();
    if registry.iter().flatten().any(|entry| {
        entry.irq == irq && entry.handler as usize == handler as usize && entry.context == context
    }) {
        return false;
    }
    let Some(slot) = registry.iter_mut().find(|slot| slot.is_none()) else {
        return false;
    };
    *slot = Some(Entry {
        irq,
        handler,
        context,
    });
    true
}

/// 冻结启动期 registry；之后只能分发，不能再注册。
pub fn freeze() {
    FROZEN.store(true, Ordering::Release);
}

/// 分发一个已由平台控制器 claim 的 IRQ。
pub fn dispatch(irq: IrqNumber) -> IrqHandled {
    let entries = *REGISTRY.lock();
    let mut handled = false;
    for entry in entries.into_iter().flatten().filter(|entry| entry.irq == irq) {
        // SAFETY: entries only become visible after the caller's boot-time
        // registration and remain immutable after `freeze`.
        if unsafe { (entry.handler)(irq, entry.context) } == IrqHandled::Yes {
            handled = true;
        }
    }
    if handled {
        HANDLED.fetch_add(1, Ordering::Relaxed);
        IrqHandled::Yes
    } else {
        SPURIOUS.fetch_add(1, Ordering::Relaxed);
        IrqHandled::No
    }
}

/// 返回诊断计数；普通 Final 不读取该接口。
pub fn stats() -> (u64, u64) {
    (
        HANDLED.load(Ordering::Relaxed),
        SPURIOUS.load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn test_handler(_irq: IrqNumber, context: usize) -> IrqHandled {
        if context == 0x51 {
            IrqHandled::Yes
        } else {
            IrqHandled::No
        }
    }

    #[test]
    fn dispatch_calls_registered_handler() {
        let irq = IrqNumber(63);
        assert!(unsafe { register_handler(irq, test_handler, 0x51) });
        assert_eq!(dispatch(irq), IrqHandled::Yes);
        assert_eq!(dispatch(IrqNumber(62)), IrqHandled::No);
    }
}
