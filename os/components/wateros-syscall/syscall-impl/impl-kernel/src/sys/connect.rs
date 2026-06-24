//! `connect(2)`：发起 TCP 连接。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;
use crate::user_copy::copy_from_user_struct;

const CONNECT_WAIT_TICKS: usize = 256;

#[repr(C)]
#[derive(Copy, Clone)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16, // network byte order
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

pub(crate) fn sys_connect(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen = args.arg(2);

    if addrlen < 2 || addr_ptr == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if crate::unix_sock::is_unix_fd(fd) {
        return match crate::unix_sock::connect(fd, addr_ptr, addrlen) {
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

    if addr.sin_family != 2 {
        return UserRet::from_error(ErrNo::EAFNOSUPPORT);
    }

    let port = u16::from_be(addr.sin_port);
    let ip = addr.sin_addr;

    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let handle = socket.handle();
    let kind = match stack::socket_kind(handle) {
        Ok(kind) => kind,
        Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    match stack::socket_connect(handle, ip, port) {
        Ok(()) if matches!(kind, stack::SocketKind::Udp) => UserRet::from_success(0),
        Ok(()) if socket_fd::is_nonblocking(fd) => UserRet::from_error(ErrNo::EINPROGRESS),
        Ok(()) => wait_connected(handle),
        Err(_) => UserRet::from_error(ErrNo::ECONNREFUSED),
    }
}

fn wait_connected(handle: smoltcp::iface::SocketHandle) -> UserRet {
    for _ in 0..CONNECT_WAIT_TICKS {
        drive_network_stack();
        // socket_is_connected 使用 is_active() 在 SynSent 即返回 true，
        // 必须同时检查 may_send 确认握手已完成（Established 状态）。
        if stack::socket_is_connected(handle).unwrap_or(false)
            && stack::socket_may_send(handle).unwrap_or(false)
        {
            return UserRet::from_success(0);
        }
        task::sleep_for_ticks(1);
    }
    UserRet::from_error(ErrNo::ETIMEDOUT)
}

fn drive_network_stack() {
    match platform::timer::now_duration() {
        Ok(now) => {
            let millis = now.as_millis().min(i64::MAX as u128) as i64;
            stack::poll_at_millis(millis);
        }
        Err(_) => stack::poll(),
    }
    stack::poll_socket_events();
}
