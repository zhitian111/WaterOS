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

/// Linux 文件名分量上限；PATH_MAX 限制整条字符串，NAME_MAX 限制每个
/// `/` 之间的分量。后者必须在查询后端前检查，否则超长名字会被误报 ENOENT。
const NAME_MAX : usize = 255;

fn validate_path_components(path : &str) -> Result<(), ErrNo> {
    if path.split('/')
           .any(|component| component.len() > NAME_MAX)
    {
        Err(ErrNo::ENAMETOOLONG)
    } else {
        Ok(())
    }
}

pub(crate) fn resolve_path_at(dirfd: isize, path: &str) -> Result<String, ErrNo> {
    if path.is_empty() {
        return Err(ErrNo::ENOENT);
    }
    validate_path_components(path)?;
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
