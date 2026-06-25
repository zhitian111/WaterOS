//! `write(2)`：fd 1/2 走控制台；pipe 写端走 IPC。

extern crate alloc;

use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_block::socket_blocking_tick;
use crate::socket_fd;
use crate::user_copy::{copy_from_user, copy_from_user_struct};
use crate::vfs_util::vfs_error_to_errno;

const TCP_BULK_WRITE_YIELD_THRESHOLD : usize = 16 * 1024;
const UDP_SMALL_WRITE_YIELD_THRESHOLD : usize = 64;
const UDP_BULK_WRITE_YIELD_INTERVAL : usize = 128;
static UDP_BULK_WRITE_COUNT : AtomicUsize = AtomicUsize::new(0);

#[repr(C)]
#[derive(Clone, Copy)]
struct UserIoVec {
    base : usize,
    len : usize,
}

pub(crate) fn sys_write(args : SyscallArgs) -> UserRet {
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
    let mut kbuf = Vec::with_capacity(len);
    kbuf.resize(len, 0);
    match copy_from_user(&mut kbuf, ptr) {
        Ok(n) if n == len => {}
        _ => return UserRet::from_error(ErrNo::EFAULT),
    }
    // iozone 调试：每 128 次 write 才打一次 trace，避免刷屏淹没 readv 日志
    {
        static WRITE_COUNT : core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
        let cnt = WRITE_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if cnt % 128 == 0 {
            log::trace!("[sys_write] cnt={} fd={} len={} ptr={:#x}",
                        cnt,
                        fd,
                        len,
                        ptr);
        }
    }
    let result = write_fd(fd, &kbuf);
    let ret = match result {
        Ok(n) => {
            log::trace!("[sys_write] OK fd={} n={}", fd, n);
            UserRet::from_success(n)
        }
        Err(ErrNo::EPIPE) => {
            let _ = super::signal::raise_current_thread(ipc::signal::SIGPIPE);
            UserRet::from_error(ErrNo::EPIPE)
        }
        Err(err) => {
            log::trace!("[sys_write] ERR fd={} err={:?}",
                        fd,
                        err);
            UserRet::from_error(err)
        }
    };
    ret
}

pub(crate) fn sys_writev(args : SyscallArgs) -> UserRet {
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
    let mut out = Vec::new();
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
        let new_len = match out.len()
                               .checked_add(iov.len)
        {
            Some(v) => v,
            None => return UserRet::from_error(ErrNo::EINVAL),
        };
        if new_len > 4 * 1024 * 1024 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        let old_len = out.len();
        out.resize(new_len, 0);
        match copy_from_user(&mut out[old_len..], iov.base) {
            Ok(n) if n == iov.len => {}
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
    }

    match write_fd(fd, &out) {
        Ok(n) => UserRet::from_success(n),
        Err(ErrNo::EPIPE) => {
            let _ = super::signal::raise_current_thread(ipc::signal::SIGPIPE);
            UserRet::from_error(ErrNo::EPIPE)
        }
        Err(err) => UserRet::from_error(err),
    }
}

fn write_fd(fd : usize, buf : &[u8]) -> Result<usize, ErrNo> {
    if let Some(socket) = socket_fd::lookup(fd) {
        return match stack::socket_kind(socket.handle()) {
            Ok(stack::SocketKind::Tcp) => write_tcp_socket_blocking(fd, buf),
            Ok(stack::SocketKind::Udp) => write_udp_socket_blocking(fd, buf),
            Err(_) => Err(ErrNo::ENOTSOCK),
        };
    }
    vfs::fd::with_current_io(fd, |handle| handle.write(buf)).map_err(vfs_error_to_errno)
}

fn write_tcp_socket_blocking(fd : usize, buf : &[u8]) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let task_id = task::current_task_id().unwrap_or(0);
    loop {
        let socket = socket_fd::lookup(fd).ok_or(ErrNo::ENOTSOCK)?;
        let handle = socket.handle();
        drive_network_stack();
        let may_send = stack::socket_may_send(handle).unwrap_or(false);
        let send_capacity = stack::socket_send_capacity(handle).unwrap_or(0);
        let connected = stack::socket_is_connected(handle).unwrap_or(false);
        if may_send && send_capacity > 0 {
            let send_len = buf.len()
                              .min(send_capacity);
            match stack::socket_send(handle, &buf[..send_len]) {
                Ok(n) if n > 0 => {
                    if n >= TCP_BULK_WRITE_YIELD_THRESHOLD {
                        drive_network_stack();
                        task::yield_now();
                        drive_network_stack();
                    }
                    return Ok(n);
                }
                Ok(_) => {}
                Err(_) => {
                    return Err(ErrNo::EIO);
                }
            }
        }
        if !connected {
            return Err(ErrNo::EPIPE);
        }
        socket_blocking_tick(nonblocking, task_id)?;
    }
}

fn write_udp_socket_blocking(fd : usize, buf : &[u8]) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let task_id = task::current_task_id().unwrap_or(0);
    loop {
        let socket = socket_fd::lookup(fd).ok_or(ErrNo::ENOTSOCK)?;
        drive_network_stack();
        match stack::socket_send(socket.handle(), buf) {
            Ok(n) => {
                let should_yield = n <= UDP_SMALL_WRITE_YIELD_THRESHOLD ||
                                   UDP_BULK_WRITE_COUNT.fetch_add(1, Ordering::Relaxed) %
                                   UDP_BULK_WRITE_YIELD_INTERVAL ==
                                   0;
                if should_yield {
                    drive_network_stack();
                    task::yield_now();
                    drive_network_stack();
                }
                return Ok(n);
            }
            Err(_) => socket_blocking_tick(nonblocking, task_id)?,
        }
    }
}

fn drive_network_stack() {
    match platform::timer::now_duration() {
        Ok(now) => {
            let millis = now.as_millis()
                            .min(i64::MAX as u128) as i64;
            stack::poll_at_millis(millis);
        }
        Err(_) => stack::poll(),
    }
    stack::poll_socket_events();
}
