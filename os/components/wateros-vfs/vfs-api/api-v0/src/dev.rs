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
    /// devfs 绝对路径（如 `/dev/vda`）。
    pub path: String,
    /// 块/字符设备等粗分类。
    pub node_type: VfsDevNodeType,
    /// Linux-style major number, when supplied by the driver.
    pub major: Option<u32>,
    /// Linux-style minor number, when supplied by the driver.
    pub minor: Option<u32>,
    /// Permission bits for the device node.
    pub mode: u16,
}

/// Generation and nodes captured as one VFS device-directory view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsDevSnapshot {
    /// Software devfs generation associated with `nodes`.
    pub generation: u64,
    /// Nodes visible at that generation.
    pub nodes: Vec<VfsDevNode>,
}

/// 设备枚举与默认根块路径查询。
pub trait VfsDevInventory {
    /// 枚举当前可见的全部 devfs 节点。
    fn list_dev_nodes(&self) -> Vec<VfsDevNode>;

    /// 返回节点快照对应的软件 devfs generation。
    ///
    /// 调用方可在缓存设备目录时保存该值；generation 变化只表示软件
    /// 视图需要重新枚举，不代表硬件热插拔或 IRQ/DMA 状态已被验证。
    fn devfs_generation(&self) -> u64 { 0 }

    /// Capture a generation and node list as one logical view.
    ///
    /// Backends with an atomic devfs implementation should override this;
    /// the default preserves compatibility for older external backends.
    fn snapshot(&self) -> VfsDevSnapshot {
        VfsDevSnapshot { generation: self.devfs_generation(), nodes: self.list_dev_nodes() }
    }

    /// 根卷默认块设备路径；无可用设备时 `None`。
    fn default_root_block_path(&self) -> Option<String>;

    /// 校验 `path` 是否为已注册块设备；默认 [`VfsError::Unsupported`]。
    fn lookup_block_device_path(&self, path: &str) -> VfsResult<()> {
        let _ = path;
        Err(crate::error::VfsError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    struct TestInventory;

    impl VfsDevInventory for TestInventory {
        fn list_dev_nodes(&self) -> Vec<VfsDevNode> {
            vec![VfsDevNode { path : String::from("/dev/test"),
                              node_type : VfsDevNodeType::Character,
                              major : None,
                              minor : None,
                              mode : 0o660 }]
        }

        fn devfs_generation(&self) -> u64 { 7 }

        fn default_root_block_path(&self) -> Option<String> { None }
    }

    #[test]
    fn default_snapshot_preserves_generation_and_nodes() {
        let snapshot = TestInventory.snapshot();
        assert_eq!(snapshot.generation, 7);
        assert_eq!(snapshot.nodes[0].path, "/dev/test");
    }
}
