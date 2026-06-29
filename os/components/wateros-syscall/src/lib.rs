#![no_std]
//! WaterOS 系统调用聚合 crate。
//!
//! `api` 导出 trap 侧 v0 契约；`active_impl` 由 feature 选择具体内核分发实现。

#[cfg(feature = "api-v0")]
pub mod api {
    pub use api_v0::*;
}

#[cfg(feature = "impl-kernel")]
pub use impl_kernel as active_impl;

#[cfg(feature = "api-v0")]
pub use api_v0::SyscallDispatcher;

#[cfg(feature = "impl-kernel")]
use abi::syscall_args::SyscallArgs;

/// trap / 异常返回路径上的 syscall 分发入口。
#[cfg(feature = "impl-kernel")]
#[inline]
pub fn dispatch_syscall_from_trap(syscall_nr : usize, syscall_args : SyscallArgs) -> isize {
    active_impl::dispatch_syscall_from_trap(syscall_nr, syscall_args)
}

/// EINTR 后可重启的 syscall 号查询（O(1) 跳表路径）。
#[cfg(feature = "impl-kernel")]
#[inline]
pub fn is_restartable_syscall(syscall_nr : usize) -> bool {
    active_impl::is_restartable_syscall(syscall_nr)
}

#[cfg(feature = "impl-kernel")]
#[inline]
pub fn timer_tick(interrupted_user: bool) {
    active_impl::timer_tick(interrupted_user);
}

#[cfg(feature = "impl-kernel")]
#[inline]
pub fn deliver_pending_signal(
    frame: *mut u8,
    restart: Option<(usize, SyscallArgs)>,
) -> isize {
    active_impl::deliver_pending_signal(frame, restart)
}

#[cfg(feature = "impl-kernel")]
#[inline]
pub fn restore_signal_frame(frame: *mut u8) -> bool {
    active_impl::restore_signal_frame(frame)
}

#[inline]
pub fn raise_current_signal(signal: usize) -> bool {
    active_impl::raise_current_signal(signal)
}

#[cfg(feature = "impl-kernel")]
#[inline]
pub fn drop_reaped_task_runtime_resources(task_id: usize, aspace: usize) {
    active_impl::drop_reaped_task_runtime_resources(task_id, aspace);
}

#[cfg(feature = "impl-kernel")]
#[inline]
pub fn record_user_page_fault_handled() {
    active_impl::record_user_page_fault_handled();
}

#[cfg(feature = "impl-kernel")]
#[inline]
pub fn log_thread_bringup_stats_summary() {
    active_impl::log_thread_bringup_stats_summary();
}

/// 当前任务的 syscall 分发入口，供汇编或 C ABI 调用方使用。
#[cfg(feature = "impl-kernel")]
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_syscall_dispatch_current(syscall_nr : usize,
                                                     arg0 : usize,
                                                     arg1 : usize,
                                                     arg2 : usize,
                                                     arg3 : usize,
                                                     arg4 : usize,
                                                     arg5 : usize)
                                                     -> isize {
    let syscall_args = SyscallArgs::from_regs([arg0, arg1, arg2, arg3, arg4, arg5]);
    dispatch_syscall_from_trap(syscall_nr, syscall_args)
}
