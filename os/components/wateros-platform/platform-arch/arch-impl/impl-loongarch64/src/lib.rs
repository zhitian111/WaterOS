#![no_std]

//! LoongArch64 **架构实现**：`trap.S` / `switch.S` 与 Rust 侧 `TrapContext`、
//! `LoongArch64ArchTaskContext` 成对维护。
//!
//! ## 与 RISC-V 实现的差异（组合与路由）
//! - Trap **业务**当前在本 crate 的 `trap_entry_rust` 内部分发（经 task 运行时符号与
//!   `firmware::timer`），**未**走 `arch-api::kernel_trap` 的不透明帧路由；替换为与
//!   RISC-V 一致的「单注册入口」时需同步调整此处与链接符号。
//! - 定时器重载使用 **固件层** `set_timer` 与 arch `StableCounter` tick 的组合约定，
//!   与 `wateros-platform-firmware` 中 QEMU UART/CSR 实现配对。

use core::arch::global_asm;

global_asm!(include_str!("../asm/trap.S"));
global_asm!(include_str!("../asm/switch.S"));

pub mod interrupt;
pub mod paging;
pub mod task;
pub mod time;
pub mod trap;

pub use trap::init_trap;
