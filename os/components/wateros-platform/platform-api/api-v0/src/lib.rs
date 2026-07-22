#![no_std]

//! 平台级 **API v0**：描述启动参数、平台时间频率等与具体 ISA / 固件实现无关的 trait
//! 契约，由 `wateros-platform` 根 crate 与 `platform-impl` 选择具体类型实现。
//!
//! 与 `wateros-platform-arch-api-v0` 的边界：本 crate **不**定义 trap 帧、CSR 或页表；
//! 仅承载“板级或环境如何解释引导参数、时间基准、控制台、deadline timer、复位”等
//! 平台语义。

pub mod boot;
pub mod console;
pub mod reset;
pub mod time;
pub mod timer;
/// CPU bring-up and online-state contract for SMP-capable platforms.
pub mod smp;
