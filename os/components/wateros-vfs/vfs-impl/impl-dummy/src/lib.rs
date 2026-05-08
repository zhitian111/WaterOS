//! 无后端存储时的 **占位 VFS 实现**：满足 trait 形状，读路径在规范化后返回 [`VfsError::NotMounted`]，可写接口返回不支持或未挂载。
//!
//! 用于默认 feature 组合下仍可链接、自检路径逻辑；真实卷由 `impl-fs-bridge` 等替换。

#![no_std]
extern crate alloc;

use alloc::vec::Vec;

pub use api_v0::{
    normalize_absolute_path, RootRwSession, SingleRootReadView, VfsDirEntry, VfsError,
    VfsMetadata, VfsResult,
};

/// 无后端时的占位根视图：路径规范化仍可用；卷访问返回 [`VfsError::NotMounted`]。
#[derive(Debug, Clone, Copy, Default)]
pub struct DummyRootView;

impl SingleRootReadView for DummyRootView {
    // 仅校验路径形状，不访问任何卷；无后端时统一返回未挂载。
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

/// 占位可写会话。
#[derive(Debug, Clone, Copy, Default)]
pub struct DummyRwSession;

impl RootRwSession for DummyRwSession {
    // 占位：无可写后端，与「未接块设备」场景一致。
    fn write_regular_file_at_root(&mut self, _name: &str, _data: &[u8]) -> VfsResult<()> {
        Err(VfsError::Unsupported)
    }
}

/// 占位 impl 自检：只读路径经规范化后应得到 [`VfsError::NotMounted`]。
pub fn test() {
    let v = DummyRootView;
    assert!(matches!(v.read("/x"), Err(VfsError::NotMounted)));
}
