#![no_std]
//! Futex task 实现：私有 registry + `ipc-waitqueue` 阻塞/唤醒 + robust 侧表。

extern crate alloc;

mod global;
mod registry;

pub use global::*;
