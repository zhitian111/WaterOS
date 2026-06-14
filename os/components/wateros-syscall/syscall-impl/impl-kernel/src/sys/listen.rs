//! `listen(2)`：标记 TCP socket 为监听状态。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;

pub(crate) fn sys_listen(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let _backlog = args.arg(1);

    let socket = match socket_fd::lookup(fd) {
        Some(s) => s,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    match stack::socket_listen(socket.handle()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => {
            log::warn!("[syscall] listen failed fd={} err={}", fd, e);
            UserRet::from_error(ErrNo::EINVAL)
        }
    }
}
