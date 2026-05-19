//! 设备节点枚举（VFS 侧视图，不暴露底层 devfs 类型）。

extern crate alloc;

use alloc::{string::String, vec::Vec};

use crate::error::VfsResult;

/// devfs 节点粗分类（VFS 命名空间）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsDevNodeType {
    Block,
    Character,
    Unsupported,
}

/// 单条设备节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsDevNode {
    pub path: String,
    pub node_type: VfsDevNodeType,
}

/// 设备枚举与默认根块路径查询。
pub trait VfsDevInventory {
    fn list_dev_nodes(&self) -> Vec<VfsDevNode>;

    fn default_root_block_path(&self) -> Option<String>;

    fn lookup_block_device_path(&self, path: &str) -> VfsResult<()> {
        let _ = path;
        Err(crate::error::VfsError::Unsupported)
    }
}
