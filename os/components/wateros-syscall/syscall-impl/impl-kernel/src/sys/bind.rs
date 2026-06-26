//! `bind(2)`：将 socket 绑定到本地地址。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;
use crate::user_copy::copy_from_user_struct;

const AF_UNSPEC: u16 = 0;
const AF_INET: u16 = 2;

#[repr(C)]
#[derive(Copy, Clone)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16, // network byte order
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

pub(crate) fn sys_bind(args: SyscallArgs) -> UserRet {
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

    if addrlen < 16 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let addr: SockAddrIn = match copy_from_user_struct(addr_ptr) {
        Ok(a) => a,
        Err(e) => return UserRet::from_error(e),
    };

    if addr.sin_family != AF_INET && addr.sin_family != AF_UNSPEC {
        return UserRet::from_error(ErrNo::EAFNOSUPPORT);
    }

    let port = u16::from_be(addr.sin_port);
    if port != 0 && port < 1024 && cred::current_credentials().effective_uid.0 != 0 {
        return UserRet::from_error(ErrNo::EACCES);
    }
    let local_ip = if addr.sin_addr == [0; 4] {
        None
    } else {
        Some(addr.sin_addr)
    };
    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    match stack::socket_bind(socket.handle(), local_ip, port) {
        Ok(()) => {
            UserRet::from_success(0)
        }
        Err("address not available") => UserRet::from_error(ErrNo::EADDRNOTAVAIL),
        Err(_) => UserRet::from_error(ErrNo::EADDRINUSE),
    }
}
