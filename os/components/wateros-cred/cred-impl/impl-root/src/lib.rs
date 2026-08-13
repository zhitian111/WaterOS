#![no_std]
#![allow(static_mut_refs)]
//! impl-root：初始 root + privileged set*id 策略。
//!
//! 状态存储与凭证策略位于 [`registry`]，面向上层的生命周期 hook 位于 [`hooks`]。

mod hooks;
mod registry;

pub use hooks::*;
pub use registry::PerTaskCredRegistry;
