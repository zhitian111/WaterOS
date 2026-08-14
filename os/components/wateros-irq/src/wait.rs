//! 等待设备完成时的本地中断控制。
//!
//! 驱动在等待外部 IRQ 完成时需要临时打开本地全局中断让中断投递，但**不能**让
//! 定时器/软中断在窗口内抢占——调用方可能持有 FS 层自旋锁，被抢占会导致其它
//! 核永久自旋。本模块提供「仅外部中断可投递」的等待窗口：保存当前状态 → 屏蔽
//! timer/soft → 打开全局中断 → 执行等待 → 恢复。
//!
//! 具体 CSR 操作由内核注册（`set_local_irq_ops`），本 crate 保持架构无关。

use core::sync::atomic::{AtomicUsize, Ordering};

type IrqSaveFn = fn() -> usize;
type IrqRestoreFn = fn(usize);

static SAVE_FN : AtomicUsize = AtomicUsize::new(0);
static RESTORE_FN : AtomicUsize = AtomicUsize::new(0);

fn noop_save() -> usize { 0 }
fn noop_restore(_state : usize) {}

/// 注册「保存并仅开外部中断」/「恢复」回调（由内核在启动时设置）。
pub fn set_local_irq_ops(save : IrqSaveFn, restore : IrqRestoreFn) {
    SAVE_FN.store(save as usize, Ordering::Release);
    RESTORE_FN.store(restore as usize, Ordering::Release);
}

fn save_fn() -> IrqSaveFn {
    let value = SAVE_FN.load(Ordering::Acquire);
    if value == 0 {
        return noop_save;
    }
    // SAFETY: 只接受 `set_local_irq_ops` 存入的合法函数指针。
    unsafe { core::mem::transmute::<usize, IrqSaveFn>(value) }
}

fn restore_fn() -> IrqRestoreFn {
    let value = RESTORE_FN.load(Ordering::Acquire);
    if value == 0 {
        return noop_restore;
    }
    // SAFETY: 只接受 `set_local_irq_ops` 存入的合法函数指针。
    unsafe { core::mem::transmute::<usize, IrqRestoreFn>(value) }
}

/// 在「仅外部中断可投递」窗口内执行 `f`，结束后恢复调用前中断状态。
pub fn with_external_only_interrupts<R>(f : impl FnOnce() -> R) -> R {
    let state = save_fn()();
    let result = f();
    restore_fn()(state);
    result
}
