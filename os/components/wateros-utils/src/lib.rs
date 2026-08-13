#![no_std]
//! 与内核策略无关的 `no_std` 纯工具聚合入口。
//!
//! 目前仅导出 [`table_format`]。它不访问串口、终端或全局状态，调用者负责
//! 决定将格式化结果写往哪里。与 CPU、MMU、启动相关的汇编必须留在
//! `wateros-platform`，不能反向放入本工具 crate。

/// `no_std`、无堆分配的文本表格格式化 crate 原样重导出。
pub use table_format;

#[cfg(feature = "self_test")]
pub fn self_test() {
    assert!(!core::any::type_name::<usize>().is_empty());
}
