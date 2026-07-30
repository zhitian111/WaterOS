#![no_std]
//! WaterOS 系统调用 facade：导出 v0 的调用号与边界类型，以及内核实现入口。

#[cfg(feature = "api-v0")]
pub use api_v0::*;

#[cfg(feature = "impl-kernel")]
pub use impl_kernel::{
    deliver_pending_signal, dispatch_syscall_from_trap, drop_reaped_task_runtime_resources,
    is_restartable_syscall, log_thread_bringup_stats_summary, ltp_submit_skip_basenames,
    raise_current_signal, record_user_page_fault_handled, restore_signal_frame,
    terminate_current_process, timer_tick,
};
