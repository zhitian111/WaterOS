//! Loongson 2K1000LA 板级平台 profile。
//!
//! 内核裸镜像由 U-Boot `go` 进入，参数解析同时接受显式 DTB 地址和 U-Boot
//! `argc`/`argv` 传参；内存、串口、PM、reset 与外部中断按 2K1000 板级约定实现。

#![no_std]

#[cfg(target_arch = "loongarch64")]
use core::arch::global_asm;

#[cfg(target_arch = "loongarch64")]
global_asm!(include_str!("asm/_start.S"));

pub mod boot;
pub mod console;
pub mod dtb;
pub mod irq;
pub mod memory;
pub mod reset;
pub mod smp;
pub mod time;
#[cfg(target_arch = "loongarch64")]
pub mod timer;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[platform/impl-loongson2k1000la] self_test begin");
    assert!(memory::physical_ram_end_exclusive() > 0);
    log::info!("[platform/impl-loongson2k1000la] self_test complete");
}
