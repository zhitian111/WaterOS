#![no_std]

//! QEMU `virt` 机器上 **RISC-V + OpenSBI** 的板级约定：`a0`/`a1` 分别承载 hart id
//! 与 DTB 物理地址等常见调用约定，时间频率当前为常量（可后续改为读 DTB）。
//!
//! 本 crate 属于 **platform-impl**：描述运行环境假设，不包含 ISA 细节实现
//!（见 `wateros-platform-arch-impl-riscv64`），但会接线该平台 profile 使用的
//! OpenSBI console、timer 与 reset 后端。
//!
//! 本 profile 的 `smp` 只负责 SBI HSM、SBI IPI 与 remote fence 等固件运输；
//! `sip`/`sie` 的本地位操作属于 `wateros-platform-arch-impl-riscv64`。

use core::arch::global_asm;

// 平台 shim 只解释 OpenSBI 参数；栈与普通启动流程由 arch boot 汇编负责。
global_asm!(include_str!("asm/_start.S"));

/// OpenSBI boot arguments and their typed view.
pub mod boot;
pub mod console;
/// 平台持有的引导 DTB 指针。
pub mod dtb;
pub mod external_irq;
/// QEMU RISC-V 平台物理内存布局解析。
pub mod memory;
/// OpenSBI system reset 后端。
pub mod reset;
/// SBI HSM based secondary-hart control for QEMU RISC-V.
pub mod smp;
/// QEMU RISC-V timebase-frequency fallback.
pub mod time;
/// OpenSBI timer 后端（经 SBI 设置下次中断时刻）。
pub mod timer;
