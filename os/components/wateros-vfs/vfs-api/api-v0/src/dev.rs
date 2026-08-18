//! 设备节点枚举（VFS 侧视图，不暴露底层 devfs 类型）。

extern crate alloc;

use alloc::{string::String, vec::Vec};

use crate::error::VfsResult;

/// devfs 节点粗分类（VFS 命名空间）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsDevNodeType {
    /// 块设备节点。
    Block,
    /// 字符设备节点。
    Character,
    /// 已枚举但未绑定实现的占位节点。
    Unsupported,
}

/// 单条设备节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsDevNode {
    /// devfs 绝对路径（如 `/dev/vda`）。
    pub path: String,
    /// 块/字符设备等粗分类。
    pub node_type: VfsDevNodeType,
}

/// 设备枚举与默认根块路径查询。
pub trait VfsDevInventory {
    /// 枚举当前可见的全部 devfs 节点。
    fn list_dev_nodes(&self) -> Vec<VfsDevNode>;

    /// 根卷默认块设备路径；无可用设备时 `None`。
    fn default_root_block_path(&self) -> Option<String>;

    /// 校验 `path` 是否为已注册块设备；默认 [`VfsError::Unsupported`]。
    fn lookup_block_device_path(&self, path: &str) -> VfsResult<()> {
        let _ = path;
        Err(crate::error::VfsError::Unsupported)
    }
}
