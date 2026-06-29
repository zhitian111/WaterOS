//! 多挂载命名空间（远期；当前仅占位契约）。

use crate::error::{VfsError, VfsResult};
use crate::kind::VfsFsKind;

/// 挂载点注册与路径前缀路由。
pub trait VfsMountTable {
    /// 在 `mount_point` 挂载指定种类辅助卷；占位默认 [`VfsError::Unsupported`]。
    fn mount_at(&mut self, mount_point: &str, kind: VfsFsKind) -> VfsResult<()> {
        let _ = (mount_point, kind);
        Err(VfsError::Unsupported)
    }

    /// 卸载 `mount_point` 上的辅助卷。
    fn unmount_at(&mut self, mount_point: &str) -> VfsResult<()> {
        let _ = mount_point;
        Err(VfsError::Unsupported)
    }

    /// 将绝对路径解析为挂载内相对前缀；未命中时由实现决定错误语义。
    fn resolve_mount(&self, path: &str) -> VfsResult<&str> {
        let _ = path;
        Err(VfsError::Unsupported)
    }
}
