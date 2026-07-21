//! `getdents64(2)`：目录 fd 枚举；`linux_dirent64` 布局由 VFS `DirectoryHandle` 编码。

//! 本模块代码由AI完成
extern crate alloc;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::fallible_buf::{try_kbuf, GETDENTS64_MAX};
use crate::user_copy::copy_to_user;
use crate::vfs_util::vfs_error_to_errno;

// 本方法代码由AI完成
pub(crate) fn sys_getdents64(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let dirp = args.arg(1);
    let count = args.arg(2);
    if dirp == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if count == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let count = count.min(isize::MAX as usize);
    let mut kbuf = match try_kbuf(count, GETDENTS64_MAX) {
        Ok(buf) => buf,
        Err(err) => return UserRet::from_error(err),
    };

    let written = match vfs::fd::with_current_io(fd, |handle| {
        if handle
            .directory_path()
            .is_none()
        {
            return Err(VfsError::NotAFile);
        }
        handle.fill_getdents64(&mut kbuf)
    }) {
        Ok(n) => n,
        Err(VfsError::NotAFile) => return UserRet::from_error(ErrNo::ENOTDIR),
        Err(VfsError::Unsupported) => return UserRet::from_error(ErrNo::ENOTDIR),
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };

    if written == 0 {
        return UserRet::from_success(0);
    }
    match copy_to_user(dirp, &kbuf[..written]) {
        Ok(n) if n == written => UserRet::from_success(written),
        _ => UserRet::from_error(ErrNo::EFAULT),
    }
}
