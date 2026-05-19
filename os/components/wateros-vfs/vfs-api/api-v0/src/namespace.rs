//! 多挂载命名空间（远期；当前仅占位契约）。

use crate::error::{VfsError, VfsResult};
use crate::kind::VfsFsKind;

/// 挂载点注册与路径前缀路由。
pub trait VfsMountTable {
    fn mount_at(&mut self, mount_point: &str, kind: VfsFsKind) -> VfsResult<()> {
        let _ = (mount_point, kind);
        Err(VfsError::Unsupported)
    }

    fn unmount_at(&mut self, mount_point: &str) -> VfsResult<()> {
        let _ = mount_point;
        Err(VfsError::Unsupported)
    }

    fn resolve_mount(&self, path: &str) -> VfsResult<&str> {
        let _ = path;
        Err(VfsError::Unsupported)
    }
}
