//! StarFive VisionFive 2 / JH7110 平台 profile 占位骨架（任务 04）。
//!
//! 当前仅提供可编译的最小平台表面（保守回退值），保证
//! `--features jh7110-visionfive2` 可 `cargo check`；真实板级实现（DTB 内存布局、
//! OpenSBI/U-Boot 启动参数、DW APB UART、PLIC 等）在任务 05 落地，本 crate 各模块
//! 将被替换内容。

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
    log::info!("[platform/impl-jh7110-visionfive2] self_test: stub skeleton only");
}
