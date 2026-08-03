//! 基于 smoltcp 的 WaterOS IPv4 协议栈实现。

#![no_std]

extern crate alloc;

mod adapter;
pub mod stack;
