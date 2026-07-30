#![no_std]
//! `wateros-klog` API v0：记录元数据、syslog action 常量与环存储 trait。
//!
//! `ARCH:` 本 crate 仅定义稳定契约，不包含全局存储、平台时间源、当前任务查询或用户内存访问。
//! 记录 ring 与锁在实现 crate，`sys_syslog` 的用户 ABI 在 syscall crate。

mod action;
mod error;
mod flags;
mod meta;
mod store;

pub use action::*;
pub use error::*;
pub use flags::*;
pub use meta::*;
pub use store::*;
