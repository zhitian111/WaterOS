//! `connect(2)`：发起 TCP 连接。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use driver::network::stack;

use crate::socket_block::socket_blocking_tick;
use crate::socket_fd;
use crate::user_copy::copy_from_user_struct;

/// 兜底避免设备丢包或协议状态异常导致阻塞 connect 永久挂起。
/// 调度 tick 当前为 10ms，约对应 30 秒。
const TCP_CONNECT_WAIT_TICKS: usize = 3_000;

#[repr(C)]
#[derive(Copy, Clone)]
// 本结构代码由AI完成
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16, // network byte order
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

// 本方法代码由AI完成
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
    let task_id = task::current_task_id().unwrap_or(0);
    for _ in 0..TCP_CONNECT_WAIT_TICKS {
        drive_network_stack();
        if stack::socket_is_connected(handle).unwrap_or(false)
            && stack::socket_may_send(handle).unwrap_or(false)
        {
            return UserRet::from_success(0);
        }
        if matches!(
            stack::socket_state(handle),
            Ok(stack::SocketState::Closed)
        ) {
            return UserRet::from_error(ErrNo::ECONNREFUSED);
        }
        if let Err(errno) = socket_blocking_tick(false, task_id) {
            return UserRet::from_error(errno);
        }
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
