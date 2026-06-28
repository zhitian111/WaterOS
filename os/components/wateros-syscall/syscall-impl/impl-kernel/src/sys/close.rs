//! `close(2)`：关闭动态 fd；pipe endpoint 会触发底层关闭。

use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::epoll_fd;
use crate::socket_fd;
use crate::vfs_util::vfs_error_to_errno;

pub(crate) fn sys_close(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let was_socket = socket_fd::lookup(fd).is_some();
    let was_unix = crate::unix_sock::is_unix_fd(fd);
    let was_epoll = epoll_fd::is_epoll_fd(fd);
    let result = vfs::fd::close_fd(fd);
    if was_socket {
        socket_fd::remove(fd);
    }
    if was_unix {
        if let Ok(task_id) = vfs::fd::current_task_id() {
            crate::unix_sock::unregister(task_id, fd);
        }
    }
    if was_epoll {
        epoll_fd::remove(fd);
    }
    match result {
        Ok(()) => UserRet::from_success(0),
        Err(err) => UserRet::from_error(vfs_error_to_errno(err)),
    }
}
