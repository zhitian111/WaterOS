//! `read(2)`：支持 pipe 读端；stdin 暂未接真实输入。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;
use wateros_base_config::task::SCHED_TIMER_PERIOD_MS;

use crate::socket_fd;
use crate::user_copy::{copy_from_user_struct, copy_to_user};
use crate::vfs_util::vfs_error_to_errno;

const SMALL_READ_BUF_SIZE: usize = 256;
const SOCKET_READ_WAIT_TICKS: usize = 128;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserIoVec {
    base: usize,
    len: usize,
}

fn finish_read(_fd: usize, ptr: usize, buf: &[u8], n: usize) -> UserRet {
    if n == 0 {
        return UserRet::from_success(0);
    }
    match copy_to_user(ptr, &buf[..n]) {
        Ok(written) if written == n => UserRet::from_success(n),
        _ => UserRet::from_error(ErrNo::EFAULT),
    }
}

pub(crate) fn sys_read(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if len > 4 * 1024 * 1024 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if len <= SMALL_READ_BUF_SIZE {
        let mut kbuf = [0u8; SMALL_READ_BUF_SIZE];
        let n = match read_fd(fd, &mut kbuf[..len]) {
            Ok(n) => n,
            Err(err) => return UserRet::from_error(err),
        };
        return finish_read(fd, ptr, &kbuf[..len], n);
    }
    let mut kbuf = Vec::with_capacity(len);
    kbuf.resize(len, 0);
    let n = match read_fd(fd, &mut kbuf) {
        Ok(n) => n,
        Err(err) => return UserRet::from_error(err),
    };
    finish_read(fd, ptr, &kbuf, n)
}

pub(crate) fn sys_readv(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    if iovcnt == 0 {
        return UserRet::from_success(0);
    }
    if iov_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if iovcnt > 1024 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut total = 0usize;
    for i in 0..iovcnt {
        let iov = match copy_from_user_struct::<UserIoVec>(iov_ptr + i * iov_size) {
            Ok(v) => v,
            Err(e) => return UserRet::from_error(e),
        };
        if iov.len == 0 {
            continue;
        }
        if iov.base == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        if iov.len > 4 * 1024 * 1024 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        if total.checked_add(iov.len).is_none() {
            return UserRet::from_error(ErrNo::EINVAL);
        }

        let mut kbuf = Vec::with_capacity(iov.len);
        kbuf.resize(iov.len, 0);
        let n = match read_fd(fd, &mut kbuf) {
            Ok(n) => n,
            Err(err) => {
                return if total > 0 {
                    UserRet::from_success(total)
                } else {
                    UserRet::from_error(err)
                };
            }
        };
        if n == 0 {
            return UserRet::from_success(total);
        }
        match copy_to_user(iov.base, &kbuf[..n]) {
            Ok(written) if written == n => {}
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
        total += n;
        if n < iov.len {
            return UserRet::from_success(total);
        }
    }

    UserRet::from_success(total)
}

fn read_fd(fd: usize, buf: &mut [u8]) -> Result<usize, ErrNo> {
    if let Some(socket) = socket_fd::lookup(fd) {
        return match stack::socket_kind(socket.handle()) {
            Ok(stack::SocketKind::Tcp) => read_tcp_socket_blocking(fd, buf),
            Ok(stack::SocketKind::Udp) => read_udp_socket_blocking(fd, buf),
            Err(_) => Err(ErrNo::ENOTSOCK),
        };
    }
    vfs::fd::with_current_io(fd, |handle| handle.read(buf)).map_err(vfs_error_to_errno)
}

fn read_tcp_socket_blocking(fd: usize, buf: &mut [u8]) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let wait_ticks = socket_recv_wait_ticks(fd, SOCKET_READ_WAIT_TICKS);
    for _ in 0..wait_ticks {
        let socket = socket_fd::lookup(fd).ok_or(ErrNo::ENOTSOCK)?;
        let handle = socket.handle();
        drive_network_stack();
        let can_recv = stack::socket_can_recv(handle).unwrap_or(false);
        let may_recv = stack::socket_may_recv(handle).unwrap_or(false);
        let state = stack::socket_state(handle);
        if can_recv {
            match stack::socket_recv(handle, buf) {
                Ok(n) => return Ok(n),
                Err(_) => return Err(ErrNo::EIO),
            }
        }
        if !may_recv {
            return Ok(0);
        }
        if matches!(state, Ok(stack::SocketState::Closed)) {
            return Ok(0);
        }
        if nonblocking {
            return Err(ErrNo::EAGAIN);
        }
        task::sleep_for_ticks(1);
    }
    Err(ErrNo::EAGAIN)
}

fn read_udp_socket_blocking(fd: usize, buf: &mut [u8]) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let wait_ticks = socket_recv_wait_ticks(fd, SOCKET_READ_WAIT_TICKS);
    for _ in 0..wait_ticks {
        let socket = socket_fd::lookup(fd).ok_or(ErrNo::ENOTSOCK)?;
        let handle = socket.handle();
        drive_network_stack();
        if stack::socket_udp_can_recv(handle).unwrap_or(false) {
            return stack::socket_recv(handle, buf).map_err(|_| ErrNo::EIO);
        }
        if nonblocking {
            return Err(ErrNo::EAGAIN);
        }
        task::sleep_for_ticks(1);
    }
    Err(ErrNo::EAGAIN)
}

fn socket_recv_wait_ticks(fd: usize, default_ticks: usize) -> usize {
    let Some(socket) = socket_fd::lookup(fd) else {
        return default_ticks;
    };
    match stack::socket_recv_timeout_ms(socket.handle()) {
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
