#![no_std]
//! 内核 syscall 实现：各 `sys_*` 具体语义，并实现 `wateros-syscall-api-v0` 的分发 trait。
//! 本模块代码由AI完成

extern crate alloc;

use api_v0::SyscallArgs;

pub const SIGBUS : usize = ipc::signal::SIGBUS;
pub const SIGILL : usize = ipc::signal::SIGILL;
pub const SIGKILL : usize = ipc::signal::SIGKILL;
pub const SIGSEGV : usize = ipc::signal::SIGSEGV;
pub const SIGTRAP : usize = ipc::signal::SIGTRAP;

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
    let result = syscall_nr_dispatch::dispatch_syscall_by_nr(syscall_nr, syscall_args);
    if result >= 0 {
        account_process_io(syscall_nr, result as u64);
    }
    result
}

/// 在 syscall 已完成且不再持有 VFS/设备锁后，统一维护 `/proc/<pid>/io`。
fn account_process_io(syscall_nr : usize, bytes : u64) {
    let direction = match syscall_nr {
        api_v0::READ | api_v0::READV | api_v0::PREAD64 |
        api_v0::PREADV | api_v0::PREADV2 => Some(true),
        api_v0::WRITE | api_v0::WRITEV | api_v0::PWRITE64 |
        api_v0::PWRITEV | api_v0::PWRITEV2 => Some(false),
        // 这些内核内搬运接口同时产生一份读流量和一份写流量。
        api_v0::SENDFILE | api_v0::SPLICE | api_v0::COPY_FILE_RANGE => {
            let Some(task_id) = task::current_task_id() else { return; };
            task::account_process_io_transfer(task_id, bytes);
            return;
        }
        _ => None,
    };
    let Some(read) = direction else { return; };
    let Some(task_id) = task::current_task_id() else { return; };
    task::account_process_io(task_id, read, bytes);
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

pub fn raise_current_fault_signal(signal : usize, code : i32, fault_addr : usize) -> bool {
    sys::raise_current_fault_signal(signal, code, fault_addr).is_ok()
}

/// Send a signal produced by the controlling terminal to its foreground group.
pub fn send_kernel_signal_to_process_group(pgid : usize, signal : usize) -> usize {
    sys::send_kernel_signal_to_process_group(task::ProcessId::from_raw(pgid), signal)
}

/// trap 等非 syscall 路径统一使用 exit_group 的资源清理与 SMP 退出流程。
pub fn terminate_current_process(exit_code : isize) -> ! {
    sys::sys_exit_group(exit_code);
    unreachable!("sys_exit_group must not return")
}

/// 以 Linux wait status 的信号终止语义结束当前线程组。
pub fn terminate_current_process_by_signal(signal : usize) -> ! {
    let task_id = task::current_task_id().unwrap_or(0);
    let exit_code = sys::task::signal_terminate_exit_code(signal, task_id);
    sys::task::exit_group_with_wait_code(exit_code);
    unreachable!("exit_group_with_wait_code must not return")
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
