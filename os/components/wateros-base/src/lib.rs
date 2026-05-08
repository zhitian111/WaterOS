#![no_std]
//! 引导阶段与内核各处共享的薄基础类型：物理/虚拟地址包装、hart ID、单核同步容器等。
//!
//! 与具体板级或内存布局相关的数值常量见独立包 `wateros-base-config`；本 crate 仅聚合类型与再导出尺度常量，不承担 MMU 策略。
pub mod addr;
pub mod boot;
pub mod config;
pub mod cpu;
pub mod sync;
