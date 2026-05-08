#![no_std]
//! 控制台 API v0：最小 [`Console`] trait，与具体固件或 QEMU 后端解耦。
//!
//! **边界**：本 crate 仅定义 trait 与类型约束，不包含任何写入实现；具体后端在 `console-impl/*` 中提供。

use core::fmt;

/// 可默认构造、且可作为 [`core::fmt::Write`] 使用的控制台后端标记 trait。
///
/// **契约**：`write_str` 等写入语义与 `fmt::Write` 一致；是否缓冲、是否换行由实现决定。
/// 当前版本未定义输入侧能力；若需读控制台，应在后续 API 版本中扩展。
///
/// 本 trait 无额外关联项，用于在类型系统中标识「可作为内核控制台」的实现，并与 `fmt` 生态对齐。
pub trait Console: fmt::Write + Default {}
