//! StarFive JH7110 VisionFive 2 板级平台 profile（任务 05 迁移自
//! `feat/visionfive2-port`，按当前 main 审计接合）。
//!
//! OpenSBI 运输层（reset/smp/timer）由 `impl-opensbi-common` 共享；入口地址、
//! DW APB UART 与物理内存布局属板级所有。真机硬件执行尚未验证。

#![no_std]

#[cfg(target_arch = "riscv64")]
use core::arch::global_asm;

#[cfg(target_arch = "riscv64")]
global_asm!(include_str!("asm/_start.S"));

pub mod boot;
pub mod console;
pub mod dtb;
pub mod memory;
#[cfg(target_arch = "riscv64")]
pub use opensbi_common::{reset, timer};
#[cfg(target_arch = "riscv64")]
pub mod smp;
pub mod time;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[platform/impl-jh7110-visionfive2] self_test begin");
    assert!(memory::physical_ram_end_exclusive() > 0);
    log::info!("[platform/impl-jh7110-visionfive2] self_test complete");
}
