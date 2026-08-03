//! 各 Linux 风格系统调用的杂项实现。

// ── 子模块 ────────────────────────────────────────────────────
pub(crate) mod acct;
pub(crate) mod bringup_stats;
pub(crate) mod ioctl;
pub(crate) mod mount;
#[cfg(target_arch = "riscv64")]
pub(crate) mod riscv_hwprobe;
pub(crate) mod sync;
pub(crate) mod sysinfo;
pub(crate) mod syslog;
pub(crate) mod umount2;

// ── 重新导出 ──────────────────────────────────────────────────
pub(crate) use acct::sys_acct;
pub(crate) use bringup_stats::{log_thread_bringup_stats_summary, record_user_page_fault_handled};
pub(crate) use ioctl::sys_ioctl;
pub(crate) use mount::sys_mount;
#[cfg(target_arch = "riscv64")]
pub(crate) use riscv_hwprobe::sys_riscv_hwprobe;
pub(crate) use sync::{sys_fdatasync, sys_fsync, sys_sync};
pub(crate) use sysinfo::{sys_getrandom, sys_sysinfo, sys_uname};
pub(crate) use syslog::sys_syslog;
pub(crate) use umount2::sys_umount2;
