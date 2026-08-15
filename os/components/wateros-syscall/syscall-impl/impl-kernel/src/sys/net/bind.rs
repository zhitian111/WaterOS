//! `bind(2)`：将 socket 绑定到本地地址。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use network::NetworkError;

use super::sockaddr::{endpoint_domain, read_bind_endpoint};
use crate::socket_fd;

// 本方法代码由AI完成
pub(crate) fn sys_bind(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen = args.arg(2);

    if addrlen < 2 || addr_ptr == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if crate::unix_sock::is_unix_fd(fd) {
        return match crate::unix_sock::bind(fd, addr_ptr, addrlen) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        };
    }

    let endpoint = match read_bind_endpoint(addr_ptr, addrlen) {
        Ok(endpoint) => endpoint,
        Err(e) => return UserRet::from_error(e),
    };
    let port = endpoint.port;
    if port != 0 &&
       port < 1024 &&
       cred::current_credentials().effective_uid
                                  .0 !=
       0
    {
        return UserRet::from_error(ErrNo::EACCES);
    }
    let local_ip = if endpoint.address
                              .is_unspecified()
    {
        None
    } else {
        Some(endpoint.address)
    };
    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };
    if endpoint_domain(endpoint) != socket.domain() {
        return UserRet::from_error(ErrNo::EAFNOSUPPORT);
    }

    match socket.bind(local_ip, port) {
        Ok(()) => UserRet::from_success(0),
        Err(NetworkError::AddressNotAvailable) => UserRet::from_error(ErrNo::EADDRNOTAVAIL),
        Err(NetworkError::AddressInUse) => UserRet::from_error(ErrNo::EADDRINUSE),
        Err(_) => UserRet::from_error(ErrNo::EADDRINUSE),
    }
}
