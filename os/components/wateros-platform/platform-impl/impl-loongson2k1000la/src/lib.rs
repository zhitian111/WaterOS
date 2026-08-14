//! Loongson 2K1000LA 平台 profile 占位骨架（任务 04）。
//!
//! 当前仅提供可编译的最小平台表面（保守回退值），保证
//! `--features loongson2k1000la` 可 `cargo check`；真实板级实现（PMON/uImage 启动、
//! DMW 段窗口、CPUCFG 时钟、NS16550 UART、PM1 reset 等）在任务 09 落地，本 crate
//! 各模块将被替换内容。

#![no_std]

pub mod boot;
pub mod console;
pub mod dtb;
pub mod memory;
pub mod reset;
pub mod smp;
pub mod time;
pub mod timer;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[platform/impl-loongson2k1000la] self_test: stub skeleton only");
}
