//! `*at` 类 syscall 共用的 `dirfd` + 相对路径解析。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::string::String;

use api_v0::ErrNo;
use vfs::api::{resolve_against_cwd, resolve_open_path, FinalSymlink, VfsError};

use crate::vfs_util::vfs_error_to_errno;

/// Linux `AT_FDCWD`。
pub(crate) const AT_FDCWD: isize = -100;

/// Linux `AT_REMOVEDIR`（`unlinkat` 删目录）。
pub(crate) const AT_REMOVEDIR: u32 = 0x200;

pub(crate) fn resolve_path_at(dirfd: isize, path: &str) -> Result<String, ErrNo> {
    if path.is_empty() {
        return Err(ErrNo::ENOENT);
    }
    if path.starts_with('/') {
        return resolve_open_path(path).map_err(super::super::super::vfs_util::vfs_error_to_errno);
    }
    if dirfd == AT_FDCWD {
        return resolve_open_path(path).map_err(super::super::super::vfs_util::vfs_error_to_errno);
    }
    if dirfd < 0 {
        return Err(ErrNo::EBADF);
    }
    let dirfd = dirfd as usize;
    let base = match vfs::fd::with_current_io(dirfd, |handle| {
        handle
            .directory_path()
            .map(|s| String::from(s))
            .ok_or(VfsError::NotAFile)
    }) {
        Ok(s) => s,
        Err(VfsError::NotAFile) => return Err(ErrNo::ENOTDIR),
        Err(e) => return Err(super::super::super::vfs_util::vfs_error_to_errno(e)),
    };
    resolve_against_cwd(base.as_str(), Some(path))
        .map_err(super::super::super::vfs_util::vfs_error_to_errno)
}

/// 展开路径中的 symlink，并将 VFS 错误转换为 syscall errno。
pub(crate) fn resolve_symlinks(
    path: &str,
    final_symlink: FinalSymlink,
) -> Result<String, ErrNo> {
    vfs::resolve_symlink_absolute(path, final_symlink).map_err(vfs_error_to_errno)
}
