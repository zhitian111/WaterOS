//! `listen(2)`：标记 TCP socket 为监听状态。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;

// 本方法代码由AI完成
pub(crate) fn sys_listen(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let backlog = args.arg(1);

    if crate::unix_sock::is_unix_fd(fd) {
        return match crate::unix_sock::listen(fd, backlog) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        };
    }

    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    match stack::socket_listen(socket.handle()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => {
            log::warn!("[syscall] listen failed fd={} err={}", fd, e);
            UserRet::from_error(ErrNo::EINVAL)
        }
    }
}
