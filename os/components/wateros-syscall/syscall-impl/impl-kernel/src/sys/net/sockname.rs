//! `getsockname(2)` / `getpeername(2)` — 获取 socket 地址。

//! 本模块代码由AI完成
use crate::socket_fd;
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

use super::sockaddr::write_endpoint;

// 本方法代码由AI完成
pub(crate) fn sys_getsockname(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);

    if crate::unix_sock::is_unix_fd(fd) {
        return match crate::unix_sock::getsockname(fd, addr_ptr, addrlen_ptr) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        };
    }

    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let endpoint = match socket.local_endpoint() {
        Ok(endpoint) => endpoint,
        Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    match write_endpoint(endpoint, addr_ptr, addrlen_ptr) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_getpeername(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);

    if crate::unix_sock::is_unix_fd(fd) {
        return match crate::unix_sock::getpeername(fd, addr_ptr, addrlen_ptr) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        };
    }

    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let endpoint = match socket.peer_endpoint() {
        Ok(endpoint) => endpoint,
        Err(_) => return UserRet::from_error(ErrNo::ENOTCONN),
    };

    match write_endpoint(endpoint, addr_ptr, addrlen_ptr) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}
