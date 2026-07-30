#![no_std]

//! 未选择实际硬件 profile 时使用的不可启动占位实现。
//!
//! 每项能力都独立放在同名模块中，避免 placeholder 行为与真实 profile 混在一起。

pub mod boot;
pub mod console;
pub mod reset;
pub mod smp;
pub mod time;
pub mod timer;
