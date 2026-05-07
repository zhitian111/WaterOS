#![no_std]
//! 控制台 API v0：最小 [`Console`] trait，与具体固件或 QEMU 后端解耦。

use core::fmt;

/// 可默认构造、且可作为 [`core::fmt::Write`] 使用的控制台后端。
///
/// **契约**：`write_str` 等写入语义与 `fmt::Write` 一致；是否缓冲、是否换行由实现决定。
/// 当前版本未定义输入侧能力；若需读控制台，应在后续 API 版本中扩展。
pub trait Console: fmt::Write + Default {}
