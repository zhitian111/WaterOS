//! `recvfrom(2)`：接收 TCP/UDP 数据。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;
use wateros_base_config::task::SCHED_TIMER_PERIOD_MS;

use crate::socket_fd;
use crate::user_copy::{copy_to_user, copy_to_user_struct};

const SOCKET_RECV_WAIT_TICKS: usize = 256;

#[repr(C)]
#[derive(Copy, Clone)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

pub(crate) fn sys_recvfrom(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let buf_ptr = args.arg(1);
    let len = args.arg(2);
    let _flags = args.arg(3);
    let addr_ptr = args.arg(4);
    let addrlen_ptr = args.arg(5);

    if len == 0 {
        return UserRet::from_success(0);
    }
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let socket = match socket_fd::lookup(fd) {
        Some(s) => s,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };
    let handle = socket.handle();

    // 根据 socket 类型选择 recv 或 recvfrom
    match stack::socket_kind(handle) {
        Ok(driver::network::stack::SocketKind::Tcp) => {
            let mut kbuf = alloc::vec![0u8; len];
            match recv_tcp_blocking(fd, handle, &mut kbuf) {
                Ok(n) if n > 0 => {
                    match copy_to_user(buf_ptr, &kbuf[..n]) {
                        Ok(written) if written == n => UserRet::from_success(n),
                        _ => UserRet::from_error(ErrNo::EFAULT),
                    }
                }
                Ok(_) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        Ok(driver::network::stack::SocketKind::Udp) => {
            let mut kbuf = alloc::vec![0u8; len];
            match recv_udp_blocking(fd, handle, &mut kbuf) {
                Ok((n, ip, port)) if n > 0 => {
                    if copy_to_user(buf_ptr, &kbuf[..n]).is_err() {
                        return UserRet::from_error(ErrNo::EFAULT);
                    }
                    if addr_ptr != 0 && addrlen_ptr != 0 {
                        if let Ok(addrlen_val) =
                            crate::user_copy::copy_from_user_struct::<u32>(addrlen_ptr)
                        {
                            let sockaddr = SockAddrIn {
                                sin_family: 2,
                                sin_port: port.to_be(),
                                sin_addr: ip,
                                sin_zero: [0; 8],
                            };
                            let write_len: usize =
                                core::mem::size_of::<SockAddrIn>().min(addrlen_val as usize);
                            let addr_bytes = unsafe {
                                core::slice::from_raw_parts(
                                    &sockaddr as *const SockAddrIn as *const u8,
                                    write_len,
                                )
                            };
                            let _ = copy_to_user(addr_ptr, addr_bytes);
                            let _ = copy_to_user_struct(addrlen_ptr, &(write_len as u32));
                        }
                    }
                    UserRet::from_success(n)
                }
                Ok(_) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        _ => UserRet::from_error(ErrNo::ENOTSOCK),
    }
}

fn recv_tcp_blocking(
    fd: usize,
    handle: smoltcp::iface::SocketHandle,
    buf: &mut [u8],
) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let wait_ticks = socket_recv_wait_ticks(handle, SOCKET_RECV_WAIT_TICKS);
    for _ in 0..wait_ticks {
        drive_network_stack();
        if stack::socket_can_recv(handle).unwrap_or(false) {
            return stack::socket_recv(handle, buf).map_err(|_| ErrNo::EIO);
        }
        if matches!(stack::socket_state(handle), Ok(stack::SocketState::Closed)) {
            return Ok(0);
        }
        if nonblocking {
            return Err(ErrNo::EAGAIN);
        }
        task::sleep_for_ticks(1);
    }
    Err(ErrNo::EAGAIN)
}

fn recv_udp_blocking(
    fd: usize,
    handle: smoltcp::iface::SocketHandle,
    buf: &mut [u8],
) -> Result<(usize, [u8; 4], u16), ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let wait_ticks = socket_recv_wait_ticks(handle, SOCKET_RECV_WAIT_TICKS);
    for _ in 0..wait_ticks {
        drive_network_stack();
        if stack::socket_udp_can_recv(handle).unwrap_or(false) {
            return stack::socket_recvfrom(handle, buf).map_err(|_| ErrNo::EIO);
        }
        if nonblocking {
            return Err(ErrNo::EAGAIN);
        }
        task::sleep_for_ticks(1);
    }
    Err(ErrNo::EAGAIN)
}

fn socket_recv_wait_ticks(handle: smoltcp::iface::SocketHandle, default_ticks: usize) -> usize {
    match stack::socket_recv_timeout_ms(handle) {
        Ok(Some(ms)) => {
            let tick_ms = (SCHED_TIMER_PERIOD_MS as u64).max(1);
            let ticks = ms.saturating_add(tick_ms - 1) / tick_ms;
            usize::try_from(ticks).unwrap_or(usize::MAX).max(1)
        }
        _ => default_ticks,
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
