//! `dup`/`dup3` 系统调用实现。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::vfs_util::vfs_error_to_errno;

/// Linux `O_CLOEXEC`（`dup3` flags）。
const O_CLOEXEC: usize = 0o2000000;

/// `dup(oldfd)` — 复制 fd 到最低可用编号。
pub(crate) fn sys_dup(args: SyscallArgs) -> UserRet {
    let oldfd = args.arg(0);
    match vfs::fd::dup_fd(oldfd, 0) {
        Ok(newfd) => UserRet::from_success(newfd),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

/// `dup3(oldfd, newfd, flags)` — 复制 fd 到指定编号。
pub(crate) fn sys_dup3(args: SyscallArgs) -> UserRet {
    let oldfd = args.arg(0);
    let newfd = args.arg(1);
    let flags = args.arg(2);
    if flags & !O_CLOEXEC != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let cloexec = (flags & O_CLOEXEC) != 0;
    match vfs::fd::dup3_fd(oldfd, newfd, cloexec) {
        Ok(fd) => UserRet::from_success(fd),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
