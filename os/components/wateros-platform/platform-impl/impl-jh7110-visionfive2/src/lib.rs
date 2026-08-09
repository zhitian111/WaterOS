#![no_std]

//! StarFive JH7110 VisionFive 2 board profile.
//!
//! OpenSBI transports are shared, while the entry address, DW APB UART and
//! physical layout are board-owned. Hardware execution remains to be tested.

#[cfg(target_arch = "riscv64")]
use core::arch::global_asm;

#[cfg(target_arch = "riscv64")]
global_asm!(include_str!("asm/_start.S"));

pub mod boot;
pub mod console;
pub mod dtb;
pub mod memory;
#[cfg(target_arch = "riscv64")]
pub use opensbi_common::{reset, smp, timer};
pub mod time;
