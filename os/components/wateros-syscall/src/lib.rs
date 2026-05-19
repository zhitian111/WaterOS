#![no_std]
#![allow(static_mut_refs)]
//! 用户态系统调用分发：将 ABI 规定的调用号与寄存器参数映射到 `sys::*` 实现。
//!
//! **分层**：[`dispatch`] 负责号表路由与 C ABI 入口；[`sys`] 承载各 `sys_*`
//! 语义实现；[`vfs_util`]、[`mm_util`] 提供 VFS fd 与内存相关辅助。
//!
//! **契约**：[`dispatch_syscall_from_trap`] 为 Rust 侧 trap 组合入口；
//! [`__wateros_syscall_dispatch_current`] 供 C ABI（如 `switch` 桩）使用。
//! 返回值遵循 `UserRet`/`ErrNo` 编码。
//!
//! **依赖**：`wateros-ipc`、`wateros-mm`；fd 表经 `wateros-vfs`（`fd-session`）；
//! `abi` / `task` 由 feature（`impl-riscv64`、`impl-loongarch64`）选择平台表与调度。

extern crate alloc;

#[cfg(feature = "fd-session")]
extern crate vfs;

mod dispatch;
mod mm_util;
mod sys;
#[cfg(feature = "fd-session")]
mod vfs_util;

pub use dispatch::dispatch_syscall_from_trap;
pub use dispatch::__wateros_syscall_dispatch_current;
