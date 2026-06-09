//! 根级可写会话（与只读根视图分离）。

use crate::error::{VfsError, VfsResult};

/// 可写会话：由 [`crate::mount::VfsMountOps::mount_rw_session`] 等创建。
pub trait RootRwSession {
    /// 在根目录下创建或覆盖普通文件；`name` 不得含 `/`。
    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> VfsResult<()>;

    fn write_regular_file(&mut self, path: &str, data: &[u8]) -> VfsResult<()> {
        let _ = (path, data);
        Err(VfsError::Unsupported)
    }

    fn unlink(&mut self, path: &str) -> VfsResult<()> {
        let _ = path;
        Err(VfsError::Unsupported)
    }

    /// 删除空目录（`unlinkat` + `AT_REMOVEDIR`）。
    fn rmdir(&mut self, path: &str) -> VfsResult<()> {
        let _ = path;
        Err(VfsError::Unsupported)
    }

    /// 从 `offset` 起写入 `data`；返回实际写入字节数。
    fn write_range(&mut self, path: &str, offset: u64, data: &[u8]) -> VfsResult<usize> {
        let _ = (path, offset, data);
        Err(VfsError::Unsupported)
    }

    /// 在绝对路径 `path` 处创建目录。
    fn mkdir(&mut self, path: &str, mode: u32) -> VfsResult<()> {
        let _ = (path, mode);
        Err(VfsError::Unsupported)
    }

    /// 将 `old_path` 重命名为 `new_path`（实现可限制为同父目录）。
    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let _ = (old_path, new_path);
        Err(VfsError::Unsupported)
    }

    /// 为 `existing_path` 创建硬链接 `new_path`。
    fn hardlink(&mut self, existing_path: &str, new_path: &str) -> VfsResult<()> {
        let _ = (existing_path, new_path);
        Err(VfsError::Unsupported)
    }
}
