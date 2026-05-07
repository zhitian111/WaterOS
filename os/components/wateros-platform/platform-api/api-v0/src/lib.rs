#![no_std]

//! 平台级 **API v0**：描述启动参数、平台时间频率等与具体 ISA / 固件实现无关的 trait
//! 契约，由 `wateros-platform` 根 crate 与 `platform-impl` 选择具体类型实现。
//!
//! 与 `wateros-platform-arch-api-v0` / `wateros-platform-firmware-api-v0` 的边界：
//! 本 crate **不**定义 trap 帧、SBI 控制台等；仅承载“板级或环境如何解释引导参数、
//! 时间基准”等平台语义。

pub mod boot;
pub mod time;
