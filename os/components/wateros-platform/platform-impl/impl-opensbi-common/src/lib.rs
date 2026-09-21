//! 板级无关的 OpenSBI 运输层，供 RISC-V 机器 profile 共享。
//!
//! 本 crate 只承载固件 ABI 操作（HSM、IPI、remote fence、timer、system reset）；
//! UART 地址、内存布局、链接地址与 DTB 解释仍归各板级 profile 所有。

#![no_std]

pub mod reset;
pub mod smp;
pub mod timer;
