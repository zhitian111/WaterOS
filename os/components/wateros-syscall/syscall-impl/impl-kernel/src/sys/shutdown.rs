//! `shutdown(2)` — 极简存根。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;

pub(crate) fn sys_shutdown(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let how = args.arg(1);

    if how > 2 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let socket = match socket_fd::lookup(fd) {
        Some(s) => s,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    match stack::socket_shutdown(socket.handle()) {
        Ok(()) => UserRet::from_success(0),
        Err("shutdown unsupported for udp") => UserRet::from_error(ErrNo::EOPNOTSUPP),
        Err(_) => UserRet::from_error(ErrNo::EIO),
    }
}
