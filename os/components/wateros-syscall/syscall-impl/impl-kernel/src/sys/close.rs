//! `close(2)`：关闭动态 fd；pipe endpoint 会触发底层关闭。

use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::socket_fd;
use crate::vfs_util::vfs_error_to_errno;

pub(crate) fn sys_close(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let was_socket = socket_fd::lookup(fd).is_some();
    let result = vfs::fd::close_fd(fd);
    if was_socket {
        socket_fd::remove(fd);
    }
    match result {
        Ok(()) => UserRet::from_success(0),
        Err(err) => UserRet::from_error(vfs_error_to_errno(err)),
    }
}
