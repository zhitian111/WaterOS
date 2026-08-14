#![no_std]
//! 内核 syscall 实现：各 `sys_*` 具体语义，并实现 `wateros-syscall-api-v0` 的分发 trait。
//! 本模块代码由AI完成

extern crate alloc;

use api_v0::SyscallArgs;

mod epoll_fd;
mod fallible_buf;
mod linux_stat;
mod mm_util;
mod poll_engine;
mod socket_block;
mod socket_fd;
mod sys;
mod syscall_nr_dispatch;
mod unix_sock;
mod user_copy;
mod vfs_util;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[syscall/impl-kernel] self_test begin");
    assert!(is_restartable_syscall(api_v0::READ));
    assert!(!is_restartable_syscall(api_v0::YIELD));
    assert!(!is_restartable_syscall(usize::MAX));
    sys::self_test();
    log::info!("[syscall/impl-kernel] self_test complete");
}

/// trap / 异常返回路径上的 syscall 分发入口。
#[inline]
pub fn dispatch_syscall_from_trap(syscall_nr : usize, syscall_args : SyscallArgs) -> isize {
    sys::record_syscall();
    syscall_nr_dispatch::dispatch_syscall_by_nr(syscall_nr, syscall_args)
}

/// 当前 syscall 号是否在 EINTR 后可由 trap 层自动重启。
#[inline]
pub fn is_restartable_syscall(syscall_nr : usize) -> bool {
    syscall_nr_dispatch::is_restartable_syscall_nr(syscall_nr)
}

#[inline]
pub fn timer_tick(interrupted_user : bool) { sys::timer_tick(interrupted_user); }

#[inline]
pub fn deliver_pending_signal(frame : *mut u8, restart : Option<(usize, SyscallArgs)>) -> isize {
    match sys::deliver_pending_signal(frame, restart) {
        Ok(false) => 0,
        Ok(true) => 1,
        Err(_) => -1,
    }
}

#[inline]
pub fn restore_signal_frame(frame : *mut u8) -> bool { sys::restore_signal_frame(frame).is_ok() }

pub fn raise_current_signal(signal : usize) -> bool { sys::raise_current_thread(signal).is_ok() }

/// Send a signal produced by the controlling terminal to its foreground group.
pub fn send_kernel_signal_to_process_group(pgid : usize, signal : usize) -> usize {
    sys::send_kernel_signal_to_process_group(task::ProcessId::from_raw(pgid), signal)
}

/// trap 等非 syscall 路径统一使用 exit_group 的资源清理与 SMP 退出流程。
pub fn terminate_current_process(exit_code : isize) -> ! {
    sys::sys_exit_group(exit_code);
    unreachable!("sys_exit_group must not return")
}

/// Finish one thread of a process whose exit-group state is already published.
///
/// Unlike the task-only exit path, this runs the syscall-owned per-thread
/// resource cleanup before removing the current scheduler entity.
pub fn terminate_current_thread(exit_code : isize) -> ! {
    sys::task::exit_current_with_wait_code(exit_code);
    unreachable!("exit_current_with_wait_code must not return")
}

/// 透明转发至 `sys::drop_reaped_task_runtime_resources`。
#[inline]
pub fn drop_reaped_task_runtime_resources(task_id : usize, aspace : usize) {
    sys::drop_reaped_task_runtime_resources(task_id, aspace);
}

/// 透明转发至 `sys::record_user_page_fault_handled`。
#[inline]
pub fn record_user_page_fault_handled() { sys::record_user_page_fault_handled(); }

#[inline]
pub fn record_syscall() { sys::record_syscall(); }

/// 透明转发至 `sys::log_thread_bringup_stats_summary`。
#[inline]
pub fn log_thread_bringup_stats_summary() { sys::log_thread_bringup_stats_summary(); }

/// 查询指定 task 当前 timer slack，单位纳秒；供 procfs 暴露。
pub fn timer_slack_for_task(task_id : usize) -> u64 {
    sys::timer_slack_for_task(task_id)
}
