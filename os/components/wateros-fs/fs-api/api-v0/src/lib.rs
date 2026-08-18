#![no_std]
//! 文件系统 API（v0）契约。
//!
//! 基础类型位于 [`types`]，读写能力 trait 位于 [`traits`]，共享句柄与
//! `FsImpl` 注册包装位于 [`handles`]；本门面保持原有公开导出路径。
//! 所有 offset/len 均以字节计，具体实现必须在加法溢出、空缓冲区和未挂载状态下返回
//! 明确错误或零长度结果，不能访问越界块。

extern crate alloc;

mod handles;
mod traits;
mod types;

pub use handles::*;
pub use traits::*;
pub use types::*;
