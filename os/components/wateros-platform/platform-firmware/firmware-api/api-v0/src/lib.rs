#![no_std]

//! **固件 API v0**：控制台、定时器与复位等经固件暴露能力的 trait 与错误类型。
//!
//! 与 `wateros-platform-arch-api-v0` 正交：本 crate **不**定义 trap 帧或 CSR 读写；
//! 由 `wateros-platform-firmware` 选择具体 `firmware-impl-*` 实现这些 trait。

pub mod console;
pub mod reset;
pub mod timer;
