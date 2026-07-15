//! 访问 fd/cwd 注册表前关中断，避免 UP 抢占导致 `RefCell` 重入 panic。

/// 在关中断临界区内执行 `f`（fd/cwd 注册表互斥用）。
pub fn with_interrupt_disabled<R>(f: impl FnOnce() -> R) -> R {
    let state = arch::interrupt::read_global_interrupt_state().ok();
    let _ = arch::interrupt::disable_global_interrupt();
    let ret = f();
    if let Some(state) = state {
        let _ = arch::interrupt::restore_global_interrupt_state(state);
    }
    ret
}
