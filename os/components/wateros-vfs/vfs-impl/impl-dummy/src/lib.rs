//! 无后端存储时的 **占位 VFS 实现**：实现 [`api_v0::VfsBackend`] 全部 trait，卷访问返回未挂载/不支持。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, vec::Vec};

use api_v0::{
    RootRwSession, SingleRootReadView, VfsBackend, VfsCapability, VfsDevInventory, VfsDevNode,
    VfsDirEntry, VfsError, VfsFsKind, VfsMetadata, VfsMountOps, VfsMountTable,
    VfsOpenOps, VfsResult, normalize_absolute_path,
};

/// 占位后端：路径规范化可用；卷与挂载相关能力返回 [`VfsError::NotMounted`] 或 [`VfsError::Unsupported`]。
#[derive(Debug, Clone, Copy, Default)]
pub struct DummyBackend;

impl SingleRootReadView for DummyBackend {
    fn exists(&self, path: &str) -> VfsResult<bool> {
        let _ = normalize_absolute_path(path)?;
        Err(VfsError::NotMounted)
    }

    fn metadata(&self, path: &str) -> VfsResult<VfsMetadata> {
        let _ = normalize_absolute_path(path)?;
        Err(VfsError::NotMounted)
    }

    fn read(&self, path: &str) -> VfsResult<Vec<u8>> {
        let _ = normalize_absolute_path(path)?;
        Err(VfsError::NotMounted)
    }

    fn read_dir(&self, path: &str) -> VfsResult<Vec<VfsDirEntry>> {
        let _ = normalize_absolute_path(path)?;
        Err(VfsError::NotMounted)
    }
}

impl VfsMountOps for DummyBackend {
    fn supported_capabilities(&self) -> Vec<VfsCapability> {
        Vec::new()
    }

    fn mount_rw_session(&self, _kind: VfsFsKind) -> VfsResult<Box<dyn RootRwSession>> {
        Err(VfsError::Unsupported)
    }
}

impl VfsDevInventory for DummyBackend {
    fn list_dev_nodes(&self) -> Vec<VfsDevNode> {
        Vec::new()
    }

    fn default_root_block_path(&self) -> Option<alloc::string::String> {
        None
    }
}

impl VfsOpenOps for DummyBackend {}
impl VfsMountTable for DummyBackend {}

impl VfsBackend for DummyBackend {}

/// 占位 impl 自检。
pub fn test() {
    let b = DummyBackend;
    assert!(matches!(b.read("/x"), Err(VfsError::NotMounted)));
}
