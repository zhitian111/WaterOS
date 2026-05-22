//! `openat(2)`：经 VFS 打开 ext4 根卷文件并分配 fd。

extern crate alloc;

use alloc::string::String;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::{resolve_against_cwd, resolve_open_path, VfsError, VfsOpenOps};

use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::{linux_open_flags_to_vfs, vfs_error_to_errno};

/// Linux `AT_FDCWD`。
const AT_FDCWD : isize = -100;

fn resolve_openat_path(dirfd : isize, path : &str) -> Result<String, ErrNo> {
    if dirfd == AT_FDCWD {
        return resolve_open_path(path).map_err(vfs_error_to_errno);
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
        Err(e) => return Err(vfs_error_to_errno(e)),
    };
    resolve_against_cwd(base.as_str(), Some(path)).map_err(vfs_error_to_errno)
}

pub(crate) fn sys_openat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let flags = args.arg(2) as u32;
    let _mode = args.arg(3);

    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_openat_path(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let vf = linux_open_flags_to_vfs(flags);

    let backend = active_impl::backend();
    let handle = match backend.open(resolved.as_str(), vf) {
        Ok(h) => h,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };

    match vfs::fd::alloc_fd(handle) {
        Ok(fd) => UserRet::from_success(fd),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
