#![no_std]

//! ramfs 聚合门面，重新导出内存目录树和文件系统实现。

#[path = "tree.rs"]
mod tree;

pub use tree::*;
