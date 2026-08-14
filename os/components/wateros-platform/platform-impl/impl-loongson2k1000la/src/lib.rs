//! Loongson 2K1000LA 板级平台 profile（任务 09）。
//!
//! 启动协议为 PMON + uImage（旧分支假设的 UEFI ABI 与此板不符）；内存/串口/PM/
//! reset 按板级事实（NPUcore 参考与 BSP 对照）实现，全部保持 MIT。真机硬件执行
//! 尚未验证。

#![no_std]

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

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[platform/impl-loongson2k1000la] self_test begin");
    assert!(memory::physical_ram_end_exclusive() > 0);
    log::info!("[platform/impl-loongson2k1000la] self_test complete");
}
