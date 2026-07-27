#![no_std]
//! WaterOS 信号状态机实现。
//!
//! `registry` 负责进程/线程状态与投递，`timer` 负责三类 timer；全局锁只在本层出现。

extern crate alloc;

mod global;
mod registry;
mod state;
mod timer;

pub use global::*;
