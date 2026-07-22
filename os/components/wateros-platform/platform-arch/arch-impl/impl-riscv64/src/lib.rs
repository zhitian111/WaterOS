#![no_std]

//! RISC-V 64 位（当前为 S 态 trap 与 Sv39 等）**架构实现**：汇编入口、trap 处理、
//! `time` CSR、中断位、`satp` 与任务上下文。
//!
//! ## 与固件 / 组合层的边界
//! Trap **业务路由**（含定时器重载、调度 tick 等）在 **`wateros`** 组合层经 `arch-api::kernel_trap`
//! 注册；本 crate 的 trap 入口仅转入该路由。固件定时器由组合层经 `platform`/`firmware` 调用。

use core::arch::global_asm;

global_asm!(include_str!("../asm/trap.asm"));
global_asm!(include_str!("../asm/switch.S"));

/// 当前 CPU id 查询。
pub mod cpu;
/// `sie` / `sstatus` 级中断开关。
pub mod interrupt;
/// `satp` 读写与 `sfence.vma`。
pub mod paging;
/// 任务上下文与进入桩函数符号。
pub mod task;
/// `time` CSR 读 tick；频率查询返回不支持（由 platform 层提供 Hz）。
pub mod time;
/// Trap 向量、`TrapContext` 与 Rust 侧 `trap_entry_rust`（转入 `arch-api::kernel_trap`，见内核 `trap_handler::init`）。
pub mod trap;
/// 安装 trap 向量（`stvec`）；更完整的启动序列见调用方。
pub use trap::init_trap;
