#![no_std]

//! LoongArch64 **架构实现**：`trap.S` / `switch.S` 与 Rust 侧 `TrapContext`、
//! `LoongArch64ArchTaskContext` 成对维护。
//!
//! Trap **业务路由**（syscall 分发、定时器重载、调度 tick 等）在组合层经
//! `arch-api::kernel_trap` 注册；本 crate 只保存/恢复帧并提供 LoongArch64 原因码解码。

use core::arch::global_asm;

global_asm!(include_str!("../asm/trap.S"));
global_asm!(include_str!("../asm/switch.S"));

pub mod interrupt;
pub mod paging;
pub mod task;
pub mod time;
pub mod trap;

pub use trap::init_trap;
