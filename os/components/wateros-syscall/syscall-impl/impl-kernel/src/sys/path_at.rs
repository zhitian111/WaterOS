//! `*at` 类 syscall 共用的 `dirfd` + 相对路径解析。

extern crate alloc;

use alloc::string::String;

use abi::errno::ErrNo;
use vfs::api::{resolve_against_cwd, resolve_open_path, VfsError};

/// Linux `AT_FDCWD`。
pub(crate) const AT_FDCWD: isize = -100;

/// Linux `AT_REMOVEDIR`（`unlinkat` 删目录）。
pub(crate) const AT_REMOVEDIR: u32 = 0x200;

pub(crate) fn resolve_path_at(dirfd: isize, path: &str) -> Result<String, ErrNo> {
    if dirfd == AT_FDCWD {
        return resolve_open_path(path).map_err(super::super::vfs_util::vfs_error_to_errno);
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
        Err(e) => return Err(super::super::vfs_util::vfs_error_to_errno(e)),
    };
    resolve_against_cwd(base.as_str(), Some(path))
        .map_err(super::super::vfs_util::vfs_error_to_errno)
}
