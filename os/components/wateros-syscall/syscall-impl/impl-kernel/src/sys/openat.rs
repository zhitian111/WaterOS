//! `openat(2)`：经 VFS 打开 ext4 根卷文件并分配 fd。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::VfsOpenOps;

use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::{linux_open_flags_to_vfs, vfs_error_to_errno};

/// Linux `AT_FDCWD`。
const AT_FDCWD : isize = -100;

pub(crate) fn sys_openat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let flags = args.arg(2) as u32;
    let _mode = args.arg(3);

    if dirfd != AT_FDCWD {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let vf = linux_open_flags_to_vfs(flags);
    if flags & 0o200_000 != 0 {
        // O_DIRECTORY
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let backend = active_impl::backend();
    let handle = match backend.open(path.as_str(), vf) {
        Ok(h) => h,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };

    match vfs::fd::alloc_fd(handle) {
        Ok(fd) => UserRet::from_success(fd),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
