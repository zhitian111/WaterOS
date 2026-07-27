//! 进程间通信（IPC）相关的 syscall 实现。

pub(crate) mod eventfd;
pub(crate) mod futex;
pub(crate) mod robust;
pub(crate) mod shm;
pub(crate) mod signal;

pub(crate) use eventfd::sys_eventfd2;
pub(crate) use futex::sys_futex;
pub(crate) use robust::{robust_exit_cleanup, drop_robust_state, futex_error_to_errno,
                        robust_exit_cleanup_siblings_for_exec, sys_get_robust_list, sys_set_robust_list};
pub(crate) use shm::{sys_shmat, sys_shmctl, sys_shmdt, sys_shmget};
pub(crate) use signal::{
    abort_clone_thread_signal, abort_fork_signal, apply_signal_dispatch, deliver_pending_signal,
    drop_thread_state, ensure_current_signal_state, ensure_process_signal_state, notify_parent_sigchld,
    on_clone_thread, on_exec, on_fork, on_thread_exit, raise_current_thread, restore_signal_frame,
    sys_kill, sys_rt_sigaction, sys_rt_sigpending, sys_rt_sigprocmask, sys_rt_sigsuspend,
    sys_rt_sigtimedwait, sys_tgkill, sys_tkill, timer_tick,
};
