//! `close(2)`：关闭动态 fd；pipe endpoint 会触发底层关闭。

use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::vfs_util::vfs_error_to_errno;

pub(crate) fn sys_close(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    match vfs::fd::close_fd(fd) {
        Ok(()) => UserRet::from_success(0),
        Err(err) => UserRet::from_error(vfs_error_to_errno(err)),
    }
}
