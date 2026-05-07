//! VFS **公共 API v0**：单根只读访问、可选根级可写会话、错误与元数据类型，以及绝对路径规范化。
//!
//! 实现方（`vfs-impl-*`）提供 [`SingleRootReadView`] / [`RootRwSession`] 的具体后端；本 crate **不** 依赖块设备或具体 FS 格式。

#![no_std]
extern crate alloc;

mod path;

pub use path::{normalize_absolute_path, NormalizedPath};

use alloc::{string::String, vec::Vec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotMounted,
    NotFound,
    NotAFile,
    InvalidPath,
    NotUtf8,
    Unsupported,
    Driver,
    Corrupt,
    Io,
}

pub type VfsResult<T> = core::result::Result<T, VfsError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsNodeType {
    File,
    Directory,
    Symlink,
    Special,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsMetadata {
    pub node_type: VfsNodeType,
    pub size: u64,
    pub mode: u16,
}

/// 单根只读视图：由具体 impl（如 fs 桥接）在已挂载根卷上提供路径级访问。
pub trait SingleRootReadView {
    fn exists(&self, path: &str) -> VfsResult<bool>;

    fn metadata(&self, path: &str) -> VfsResult<VfsMetadata>;

    fn read(&self, path: &str) -> VfsResult<Vec<u8>>;

    fn read_prefix(&self, path: &str, len: usize) -> VfsResult<Vec<u8>> {
        let mut data = self.read(path)?;
        if data.len() > len {
            data.truncate(len);
        }
        Ok(data)
    }

    fn read_to_string(&self, path: &str) -> VfsResult<String> {
        let data = self.read(path)?;
        String::from_utf8(data).map_err(|_| VfsError::NotUtf8)
    }

    /// 启动期调试：默认空操作；桥接可委托底层 `ReadOnlyFs::boot_dump_all_paths`。
    fn boot_dump_all_paths(&self) {}
}

/// 可写会话：与只读根句柄分离（例如独立 `SharedRwFs`），由 impl 管理生命周期。
pub trait RootRwSession {
    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> VfsResult<()>;
}

pub fn test() {
    assert_eq!(
        normalize_absolute_path("/a/./b/../c").unwrap().as_str(),
        "/a/c"
    );
    assert_eq!(normalize_absolute_path("//").unwrap().as_str(), "/");
}
