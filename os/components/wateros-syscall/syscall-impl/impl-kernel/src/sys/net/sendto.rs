//! `sendto(2)`：UDP 发送数据报到指定地址。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use driver::network::stack;

use crate::fallible_buf::{try_kbuf, SYSCALL_IO_MAX};
use crate::socket_block::socket_blocking_tick;
use crate::socket_fd;
use crate::user_copy::{copy_from_user, copy_from_user_struct};

const TCP_BULK_SEND_YIELD_THRESHOLD: usize = 64 * 1024;
const TCP_MSS_BYTES: usize = 1460;
const TCP_LOOPBACK_POLL_ROUNDS: usize = 4;
const MSG_DONTWAIT: usize = 0x40;

#[repr(C)]
#[derive(Copy, Clone)]
// 本结构代码由AI完成
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

// 本方法代码由AI完成
pub(crate) fn sys_sendto(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let buf_ptr = args.arg(1);
    let len = args.arg(2);
    let flags = args.arg(3);
    let addr_ptr = args.arg(4);
    let addrlen = args.arg(5);

    if len == 0 {
        return UserRet::from_success(0);
    }
    if buf_ptr == 0 || len > SYSCALL_IO_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if crate::unix_sock::is_unix_fd(fd) {
        let mut kbuf = match try_kbuf(len, SYSCALL_IO_MAX) {
            Ok(buf) => buf,
            Err(err) => return UserRet::from_error(err),
        };
        match copy_from_user(&mut kbuf, buf_ptr) {
            Ok(n) if n == len => {}
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
        return match crate::unix_sock::sendto_unix(fd, &kbuf, addr_ptr, addrlen) {
            Ok(n) => UserRet::from_success(n),
            Err(e) => UserRet::from_error(e),
        };
    }

    let socket = match socket_fd::lookup(fd) {
        Some(s) => s,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };
    let handle = socket.handle();

    // 解析目标地址
    let (ip, port) = if addr_ptr != 0 && addrlen >= 16 {
        match copy_from_user_struct::<SockAddrIn>(addr_ptr) {
            Ok(addr) => (
                addr.sin_addr,
                u16::from_be(addr.sin_port),
            ),
            Err(_) => return UserRet::from_error(ErrNo::EFAULT),
        }
    } else {
        // 没有目标地址 → 作为 TCP send（或已 connect 的 UDP）
        return match send_connected_socket(fd, handle, buf_ptr, len, flags) {
            Ok(n) => UserRet::from_success(n),
            Err(err) => {
                log::warn!("[syscall] sendto connected failed: {:?}", err);
                UserRet::from_error(err)
            }
        };
    };

    let mut kbuf = match try_kbuf(len, SYSCALL_IO_MAX) {
        Ok(buf) => buf,
        Err(err) => return UserRet::from_error(err),
    };
    match copy_from_user(&mut kbuf, buf_ptr) {
        Ok(n) if n == len => {}
        _ => return UserRet::from_error(ErrNo::EFAULT),
    }

    match send_udp_blocking(fd, handle, &kbuf, Some((ip, port)), flags) {
        Ok(n) => UserRet::from_success(n),
        Err(err) => UserRet::from_error(err),
    }
}

fn send_connected_socket(
    fd: usize,
    handle: smoltcp::iface::SocketHandle,
    buf_ptr: usize,
    len: usize,
    flags: usize,
) -> Result<usize, ErrNo> {
    match stack::socket_kind(handle).map_err(|_| ErrNo::ENOTSOCK)? {
        stack::SocketKind::Tcp => send_tcp_blocking(fd, handle, buf_ptr, len, flags),
        stack::SocketKind::Udp => {
            let mut kbuf = try_kbuf(len, SYSCALL_IO_MAX)?;
            match copy_from_user(&mut kbuf, buf_ptr) {
                Ok(n) if n == len => {}
                _ => return Err(ErrNo::EFAULT),
            }
            send_udp_blocking(fd, handle, &kbuf, None, flags)
        }
    }
}

fn send_tcp_blocking(
    fd: usize,
    handle: smoltcp::iface::SocketHandle,
    buf_ptr: usize,
    len: usize,
    flags: usize,
) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd) || (flags & MSG_DONTWAIT) != 0;
    let task_id = task::current_task_id().unwrap_or(0);
    loop {
        drive_network_stack();
        let may_send = stack::socket_may_send(handle).unwrap_or(false);
        let send_capacity = stack::socket_send_capacity(handle).unwrap_or(0);
        let connected = stack::socket_is_connected(handle).unwrap_or(false);
        if may_send && send_capacity > 0 {
            let send_len = len.min(send_capacity);
            let mut kbuf = try_kbuf(send_len, SYSCALL_IO_MAX)?;
            match copy_from_user(&mut kbuf, buf_ptr) {
                Ok(n) if n == send_len => {}
                _ => return Err(ErrNo::EFAULT),
            }
            match stack::socket_send(handle, &kbuf) {
                Ok(n) if n > 0 => {
                    flush_segmented_loopback_send(handle, n);
                    if n >= TCP_BULK_SEND_YIELD_THRESHOLD {
                        drive_network_stack();
                        task::yield_now();
                        drive_network_stack();
                    }
                    return Ok(n);
                }
                Ok(_) => {}
                Err(_) => return Err(ErrNo::EIO),
            }
        }
        if !connected {
            if !may_send
                && !stack::socket_may_recv(handle).unwrap_or(false)
                && matches!(stack::socket_state(handle), Ok(stack::SocketState::Connected))
            {
                return Ok(len);
            }
            return Err(ErrNo::EPIPE);
        }
        crate::socket_block::socket_blocking_tick(nonblocking, task_id)?;
    }
}

pub(super) fn send_udp_blocking(
    fd: usize,
    handle: smoltcp::iface::SocketHandle,
    data: &[u8],
    destination: Option<([u8; 4], u16)>,
    flags: usize,
) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd) || (flags & MSG_DONTWAIT) != 0;
    let task_id = task::current_task_id().unwrap_or(0);
    loop {
        drive_network_stack();
        let result = match destination {
            Some((ip, port)) => stack::socket_sendto(handle, data, ip, port),
            None => stack::socket_send(handle, data),
        };
        match result {
            Ok(n) => {
                // 尽快把刚入队的数据报交给设备或 smoltcp 本机回灌队列。
                drive_network_stack();
                return Ok(n);
            }
            Err(stack::SocketSendError::WouldBlock) => {
                socket_blocking_tick(nonblocking, task_id)?;
            }
            Err(err) => return Err(socket_send_error_to_errno(err)),
        }
    }
}

pub(crate) fn socket_send_error_to_errno(err: stack::SocketSendError) -> ErrNo {
    match err {
        stack::SocketSendError::MessageTooLarge => ErrNo::EMSGSIZE,
        stack::SocketSendError::WouldBlock => ErrNo::EAGAIN,
        stack::SocketSendError::NoBufferSpace => ErrNo::ENOBUFS,
        stack::SocketSendError::NotConnected => ErrNo::ENOTCONN,
        stack::SocketSendError::InvalidDestination => ErrNo::EDESTADDRREQ,
        stack::SocketSendError::InvalidSocket => ErrNo::ENOTSOCK,
        stack::SocketSendError::StackUnavailable | stack::SocketSendError::Io => ErrNo::EIO,
    }
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

fn flush_segmented_loopback_send(handle : smoltcp::iface::SocketHandle, sent : usize) {
    if sent <= TCP_MSS_BYTES || !stack::socket_peer_is_loopback(handle).unwrap_or(false) {
        return;
    }
    for _ in 0..TCP_LOOPBACK_POLL_ROUNDS {
        drive_network_stack();
    }
}
