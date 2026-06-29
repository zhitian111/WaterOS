#![no_std]
//! `wateros-klog` API v0：记录元数据、syslog action 常量与环存储 trait。
//!
//! 本 crate 仅定义契约，不包含全局存储或平台时间源。
//! 本模块代码由AI完成

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
