#![no_std]

//! Loongson 2K1000LA Nebula board profile skeleton.
//!
//! UART and the conservative high-memory window are documented by the BSP.
//! Boot-parameter parsing, SMP mailbox and reset are intentionally not claimed
//! until their shipping firmware ABI has been exercised on hardware.

#[cfg(target_arch = "loongarch64")]
use core::arch::global_asm;

#[cfg(target_arch = "loongarch64")]
global_asm!(include_str!("asm/_start.S"));

pub mod boot;
pub mod console;
pub mod dtb;
pub mod memory;
pub mod reset;
pub mod smp;
pub mod time;
#[cfg(target_arch = "loongarch64")]
pub mod timer;
