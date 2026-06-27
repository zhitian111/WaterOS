//! `accept4(2)`：接受 TCP 连接并返回新 fd。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use alloc::boxed::Box;
use driver::network::socket_handles::{SocketRef, TcpStreamHandle};
use driver::network::stack;
use vfs::api::handle::VfsIoHandle;

use crate::socket_block::socket_blocking_tick;
use crate::socket_fd;
use crate::user_copy::copy_to_user_struct;

const SOCK_NONBLOCK: usize = 0o0004000;
const SOCK_CLOEXEC: usize = 0o2000000;
const FD_CLOEXEC: usize = 1;

#[repr(C)]
#[derive(Copy, Clone)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

pub(crate) fn sys_accept4(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);
    let flags = args.arg(3);
    accept_inner(fd, addr_ptr, addrlen_ptr, flags)
}

pub(crate) fn sys_accept(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);
    accept_inner(fd, addr_ptr, addrlen_ptr, 0)
}

fn accept_inner(fd: usize, addr_ptr: usize, addrlen_ptr: usize, flags: usize) -> UserRet {
    if flags & !(SOCK_NONBLOCK | SOCK_CLOEXEC) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if vfs::fd::is_path_only_fd(fd).unwrap_or(false) {
        return UserRet::from_error(ErrNo::EBADF);
    }

    if crate::unix_sock::is_unix_fd(fd) {
        return accept_unix(fd, addr_ptr, addrlen_ptr, flags);
    }

    let socket = match socket_fd::lookup(fd) {
        Some(s) => s,
        None => {
            if vfs::fd::with_current_io(fd, |_| Ok(())).is_ok() {
                return UserRet::from_error(ErrNo::ENOTSOCK);
            }
            return UserRet::from_error(ErrNo::EBADF);
        }
    };

    match stack::socket_kind(socket.handle()) {
        Ok(stack::SocketKind::Udp) => return UserRet::from_error(ErrNo::EOPNOTSUPP),
        Ok(stack::SocketKind::Tcp) => {}
        Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
    }

    let nonblocking = (flags & SOCK_NONBLOCK) != 0
        || socket_fd::is_nonblocking(fd);
    let task_id = task::current_task_id().unwrap_or(0);

    loop {
        drive_network_stack();
        match stack::socket_has_pending_accept(socket.handle()) {
            Ok(true) => break,
            Ok(false) => {
                if nonblocking {
                    return UserRet::from_error(ErrNo::EAGAIN);
                }
                if let Err(errno) = socket_blocking_tick(false, task_id) {
                    return UserRet::from_error(errno);
                }
            }
            Err("not a listening socket") => return UserRet::from_error(ErrNo::EINVAL),
            Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
        }
    }

    let (established_handle, replacement_listener, _accepted_port) = match stack::socket_accept(socket.handle()) {
        Ok(v) => v,
        Err(_) => return UserRet::from_error(ErrNo::ECONNRESET),
    };
    socket.replace_handle(replacement_listener);
    let established_socket = SocketRef::new(established_handle);

    // 为新连接分配 fd
    let io_handle: Box<dyn VfsIoHandle> = Box::new(TcpStreamHandle {
        socket: established_socket.clone(),
    });
    let new_fd = match vfs::fd::alloc_fd(io_handle) {
        Ok(fd) => fd,
        Err(_) => {
            let _ = stack::socket_close(established_handle);
            return UserRet::from_error(ErrNo::ENOMEM);
        }
    };
    if flags & SOCK_CLOEXEC != 0 {
        if vfs::fd::set_fd_flags(new_fd, FD_CLOEXEC).is_err() {
            let _ = stack::socket_close(established_handle);
            return UserRet::from_error(ErrNo::EBADF);
        }
    }
    let status_flags = if flags & SOCK_NONBLOCK != 0 {
        SOCK_NONBLOCK
    } else {
        0
    };
    socket_fd::register_with_flags(new_fd, established_socket, status_flags);

    // 写回客户端地址（如果有 addr 缓冲区）
    if addr_ptr != 0 && addrlen_ptr != 0 {
        let addr = SockAddrIn {
            sin_family: 2,          // AF_INET
            sin_port: 0u16.to_be(), // unknown client port from smoltcp
            sin_addr: [
                127, 0, 0, 1,
            ],
            sin_zero: [0; 8],
        };
        if let Ok(addrlen_val) = crate::user_copy::copy_from_user_struct::<u32>(addrlen_ptr) {
            let write_len = core::mem::size_of::<SockAddrIn>().min(addrlen_val as usize);
            let addr_bytes = unsafe {
                core::slice::from_raw_parts(
                    &addr as *const SockAddrIn as *const u8,
                    write_len,
                )
            };
            let _ = crate::user_copy::copy_to_user(addr_ptr, addr_bytes);
            let _ = copy_to_user_struct(addrlen_ptr, &(write_len as u32));
        }
    }

    UserRet::from_success(new_fd)
}

fn accept_unix(fd: usize, _addr_ptr: usize, _addrlen_ptr: usize, flags: usize) -> UserRet {
    let (io_handle, sock) = match crate::unix_sock::accept(fd) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };
    let new_fd = match vfs::fd::alloc_fd(io_handle) {
        Ok(fd) => fd,
        Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
    };
    if flags & SOCK_CLOEXEC != 0 {
        if vfs::fd::set_fd_flags(new_fd, FD_CLOEXEC).is_err() {
            return UserRet::from_error(ErrNo::EBADF);
        }
    }
    crate::unix_sock::register(new_fd, sock);
    UserRet::from_success(new_fd)
}

fn drive_network_stack() {
    match platform::timer::now_duration() {
        Ok(now) => {
            let millis = now.as_millis().min(i64::MAX as u128) as i64;
            stack::poll_burst_at_millis(millis);
        }
        Err(_) => stack::poll_burst_at_millis(0),
    }
}
