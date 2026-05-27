//! `shutdown(2)` — 极简存根。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;

pub(crate) fn sys_shutdown(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let _how = args.arg(1);

    let handle = match socket_fd::lookup(fd) {
        Some(h) => h,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    // smoltcp 的 TCP close 即是 shutdown
    match stack::socket_close(handle) {
        Ok(()) => {
            socket_fd::remove(fd);
            UserRet::from_success(0)
        }
        Err(_) => UserRet::from_error(ErrNo::EIO),
    }
}
