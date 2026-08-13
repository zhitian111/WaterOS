#![no_std]
//! 进程凭证 v0 契约。
//!
//! 基础 ID 与凭证快照位于 [`types`]；后端与权限契约位于 [`traits`]。

mod traits;
mod types;

pub use traits::*;
pub use types::*;
