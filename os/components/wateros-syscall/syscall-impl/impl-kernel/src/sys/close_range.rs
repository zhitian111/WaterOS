//! `close_range(2)`：批量关闭 fd 或批量设置 `FD_CLOEXEC`。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::socket_fd;
use crate::vfs_util::vfs_error_to_errno;

const CLOSE_RANGE_UNSHARE: usize = 1 << 1;
const CLOSE_RANGE_CLOEXEC: usize = 1 << 2;

pub(crate) fn sys_close_range(args: SyscallArgs) -> UserRet {
    let first = args.arg(0);
    let last = args.arg(1);
    let flags = args.arg(2);

    if first > last {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & !(CLOSE_RANGE_UNSHARE | CLOSE_RANGE_CLOEXEC) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & CLOSE_RANGE_UNSHARE != 0 {
        log::warn!(
            "[syscall] close_range(nr=436) CLOSE_RANGE_UNSHARE unsupported flags={:#x}",
            flags,
        );
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if flags & CLOSE_RANGE_CLOEXEC != 0 {
        return match vfs::fd::set_fd_range_cloexec(first, last, true) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    match vfs::fd::close_fd_range(first, last) {
        Ok(closed_fds) => {
            for fd in closed_fds {
                socket_fd::remove(fd);
                if let Ok(task_id) = vfs::fd::current_task_id() {
                    crate::unix_sock::unregister(task_id, fd);
                }
            }
            UserRet::from_success(0)
        }
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
