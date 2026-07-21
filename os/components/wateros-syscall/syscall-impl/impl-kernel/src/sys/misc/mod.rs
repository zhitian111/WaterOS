//! 各 Linux 风格系统调用的杂项实现。

// ── 子模块 ──────────────────────────────────────────────────────
pub(crate) mod acct;
pub(crate) mod ioctl;
pub(crate) mod ltp_cgroup_helper;
pub(crate) mod mount;
pub(crate) mod sync;
pub(crate) mod syslog;
pub(crate) mod umount2;

// ── 重新导出 ────────────────────────────────────────────────────
pub(crate) use acct::sys_acct;
pub(crate) use ioctl::sys_ioctl;
pub(crate) use mount::sys_mount;
pub(crate) use sync::{sys_fdatasync, sys_fsync, sys_sync};
pub(crate) use syslog::sys_syslog;
pub(crate) use umount2::sys_umount2;

/// bring-up 从根卷删除 LTP 排除用例时读取的 basename 表（与 fast-exit 同表）。
#[inline]
pub fn ltp_submit_skip_basenames() -> &'static [&'static str] {
    ltp_cgroup_helper::ltp_submit_skip_basenames()
}
