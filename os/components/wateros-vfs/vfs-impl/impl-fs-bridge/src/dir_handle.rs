//! 已打开目录句柄：供 `openat(dirfd, …)` 相对路径解析。

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;

use api_v0::{
    SingleRootReadView, VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsResult,
};

use crate::FsBridge;

/// 根卷上已打开的目录（只记录绝对路径，不缓存目录项）。
pub struct DirectoryHandle {
    path: String,
    meta: VfsMetadata,
}

impl DirectoryHandle {
    pub(crate) fn open(bridge: &FsBridge, path: String) -> VfsResult<Box<dyn VfsIoHandle>> {
        if !bridge.exists(path.as_str())? {
            return Err(VfsError::NotFound);
        }
        let meta = bridge.metadata(path.as_str())?;
        if meta.node_type != VfsNodeType::Directory {
            return Err(VfsError::NotAFile);
        }
        Ok(Box::new(Self { path, meta }))
    }
}

impl VfsIoHandle for DirectoryHandle {
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(self.meta.clone())
    }

    fn directory_path(&self) -> Option<&str> {
        Some(self.path.as_str())
    }

    fn read(&mut self, _buf: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::NotAFile)
    }

    fn write(&mut self, _buf: &[u8]) -> VfsResult<usize> {
        Err(VfsError::NotAFile)
    }
}
