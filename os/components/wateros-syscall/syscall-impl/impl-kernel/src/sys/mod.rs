//! 各 Linux 风格系统调用的 `sys_*` 实现；由 [`crate::dispatch_syscall_by_nr`]
//! 按号表路由。

// ── 子模块组 ────────────────────────────────────────────────────
mod fs;
mod ipc;
mod mem;
mod net;
mod poll;
mod time;

// ── 子模块组 ────────────────────────────────────────────────────
mod misc;
#[path = "../stat_times.rs"]
mod stat_times;
pub(crate) mod task;

// ── 通过子模块组重新导出 ────────────────────────────────────────
pub(crate) use fs::*;
pub(crate) use ipc::*;
pub(crate) use mem::*;
pub(crate) use misc::*;
pub(crate) use net::*;
pub(crate) use poll::*;
pub(crate) use time::*;

// ── 独立模块重新导出 ────────────────────────────────────────────
pub(crate) use task::*;
