#![no_std]

//! QEMU `virt` LoongArch64 的板级 profile。
//!
//! 本 crate 只选择并组织该机器的 boot、console、timer、reset、SMP 后端；
//! ISA 的 trap、页表和 CSR 语义仍属于 `wateros-platform-arch`。其中 mailbox 与
//! IPI 发送在本 profile；本地 IOCSR pending 清除与中断使能在 arch interrupt。

use core::arch::global_asm;

global_asm!(include_str!("asm/_start.S"));

pub mod boot;
pub mod console;
/// 平台持有的引导 DTB 指针。
pub mod dtb;
/// QEMU LoongArch64 平台物理内存布局解析。
pub mod memory;
pub mod reset;
pub mod smp;
pub mod time;
pub mod timer;
