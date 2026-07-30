#![no_std]

//! 平台级 **API v0**：描述启动参数、平台时间频率与 SMP 运输层等跨 profile 的类型和
//! 必要 trait，由 `wateros-platform` 根 crate 与 `platform-impl` 选择具体实现。
//!
//! 与 `wateros-platform-arch-api-v0` 的边界：本 crate **不**定义 trap 帧、CSR 或页表；
//! 仅承载“板级或环境如何解释引导参数、时间基准、控制台、deadline timer、复位”等
//! 平台语义。控制台、timer、reset 均以具体后端函数提供，不为无实现的空 trait
//! 保留抽象层。

pub mod boot;
pub mod console;
pub mod reset;
/// CPU bring-up and online-state contract for SMP-capable platforms.
pub mod smp;
pub mod time;
pub mod timer;
