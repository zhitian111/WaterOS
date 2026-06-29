//! `*at` 类 syscall 共用的 `dirfd` + 相对路径解析。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::string::String;

use abi::errno::ErrNo;
use vfs::api::{resolve_against_cwd, resolve_open_path, VfsError};

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
        return resolve_open_path(path).map_err(super::super::vfs_util::vfs_error_to_errno);
    }
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

/// 解析路径末端 symlink（最多 40 跳），与 Linux `open`/`access` follow 语义对齐。
pub(crate) fn resolve_final_symlink(path: &str) -> Result<String, ErrNo> {
    let mut current = String::from(path);
    for _ in 0..40 {
        let target = match vfs::read_symlink_absolute(current.as_str()) {
            Ok(target) => target,
            Err(VfsError::NotAFile) => return Ok(current),
            Err(e) => return Err(vfs_error_to_errno(e)),
        };
        let target = core::str::from_utf8(target.as_slice()).map_err(|_| ErrNo::EINVAL)?;
        let parent = current
            .rsplit_once('/')
            .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
            .unwrap_or("/");
        current = resolve_against_cwd(parent, Some(target)).map_err(vfs_error_to_errno)?;
    }
    Err(ErrNo::ELOOP)
}
