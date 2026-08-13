#![no_std]
//! WaterOS 各组件共享的最小基础类型与同步原语。
//!
//! 与具体板级、内存布局或子系统容量相关的数值常量见独立包
//! `wateros-base-config`。本 crate 不依赖 platform、task、MM 或 syscall，
//! 以免基础依赖反向引用上层子系统。
pub mod cpu;
pub mod sync;

#[cfg(feature = "self_test")]
pub fn self_test() {
    let mask = cpu::CpuMask::EMPTY;
    assert_eq!(mask.bits(), 0);
}
