//! `socket(2)`：创建 socket 并分配 fd。

extern crate alloc;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use alloc::boxed::Box;
use driver::network::socket_handles::{TcpStreamHandle, UdpSocketHandle};
use driver::network::stack;
use vfs::api::handle::VfsIoHandle;

use crate::socket_fd;

const AF_INET: usize = 2;
const SOCK_STREAM: usize = 1;
const SOCK_DGRAM: usize = 2;
const SOCK_NONBLOCK: usize = 0o4000;
const SOCK_CLOEXEC: usize = 0o2000000;

pub(crate) fn sys_socket(args: SyscallArgs) -> UserRet {
    let domain = args.arg(0);
    let mut typ = args.arg(1);
    let _protocol = args.arg(2);

    if domain != AF_INET {
        return UserRet::from_error(ErrNo::EAFNOSUPPORT);
    }

    let _cloexec = typ & SOCK_CLOEXEC != 0;
    typ &= !(SOCK_NONBLOCK | SOCK_CLOEXEC);

    let handle_result = match typ {
        SOCK_STREAM => stack::create_tcp_socket(),
        SOCK_DGRAM => stack::create_udp_socket(),
        _ => return UserRet::from_error(ErrNo::EPROTONOSUPPORT),
    };

    let smoltcp_handle = match handle_result {
        Ok(h) => h,
        Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
    };

    let io_handle: Box<dyn VfsIoHandle> = match typ {
        SOCK_STREAM => Box::new(TcpStreamHandle {
            handle: smoltcp_handle,
        }),
        SOCK_DGRAM => Box::new(UdpSocketHandle {
            handle: smoltcp_handle,
        }),
        _ => unreachable!(),
    };

    let fd = match vfs::fd::alloc_fd(io_handle) {
        Ok(fd) => fd,
        Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
    };
    socket_fd::register(fd, smoltcp_handle);

    UserRet::from_success(fd)
}
