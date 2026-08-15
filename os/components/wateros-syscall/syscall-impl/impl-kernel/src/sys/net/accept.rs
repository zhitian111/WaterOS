//! `accept4(2)`：接受 TCP 连接并返回新 fd。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use network::stack;
use network::{NetworkError, SocketKind};

use crate::socket_block::socket_blocking_tick;
use crate::socket_fd;

use super::sockaddr::write_endpoint;

const SOCK_NONBLOCK : usize = 0o0004000;
const SOCK_CLOEXEC : usize = 0o2000000;
const FD_CLOEXEC : usize = 1;

// 本方法代码由AI完成
pub(crate) fn sys_accept4(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);
    let flags = args.arg(3);
    accept_inner(fd, addr_ptr, addrlen_ptr, flags)
}

// 本方法代码由AI完成
pub(crate) fn sys_accept(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);
    accept_inner(fd, addr_ptr, addrlen_ptr, 0)
}

fn accept_inner(fd : usize, addr_ptr : usize, addrlen_ptr : usize, flags : usize) -> UserRet {
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

    match socket.kind() {
        Ok(SocketKind::Udp | SocketKind::Icmp) => {
            return UserRet::from_error(ErrNo::EOPNOTSUPP);
        }
        Ok(SocketKind::Tcp) => {}
        Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
    }

    let nonblocking = (flags & SOCK_NONBLOCK) != 0 || socket_fd::is_nonblocking(fd);
    let task_id = task::current_task_id().unwrap_or(0);

    let status_flags = if flags & SOCK_NONBLOCK != 0 {
        SOCK_NONBLOCK
    } else {
        0
    };
    let (established_socket, peer) = loop {
        drive_network_stack();
        match socket.accept(status_flags) {
            Ok(accepted) => break accepted,
            Err(NetworkError::NoPendingConnection) => {
                if nonblocking {
                    return UserRet::from_error(ErrNo::EAGAIN);
                }
                if let Err(errno) = socket_blocking_tick(false, task_id) {
                    return UserRet::from_error(errno);
                }
            }
            Err(NetworkError::NotListening) => return UserRet::from_error(ErrNo::EINVAL),
            Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
        }
    };

    // 为新连接分配 fd
    let io_handle = match established_socket.into_vfs_handle() {
        Ok(handle) => handle,
        Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
    };
    let new_fd = match vfs::fd::alloc_fd(io_handle) {
        Ok(fd) => fd,
        Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
    };
    if flags & SOCK_CLOEXEC != 0 {
        if vfs::fd::set_fd_flags(new_fd, FD_CLOEXEC).is_err() {
            let _ = vfs::fd::close_fd(new_fd);
            return UserRet::from_error(ErrNo::EBADF);
        }
    }
    // 写回客户端地址（如果有 addr 缓冲区）
    if addr_ptr != 0 {
        if let Err(error) = write_endpoint(peer, addr_ptr, addrlen_ptr) {
            let _ = vfs::fd::close_fd(new_fd);
            return UserRet::from_error(error);
        }
    }

    UserRet::from_success(new_fd)
}

fn accept_unix(fd : usize, _addr_ptr : usize, _addrlen_ptr : usize, flags : usize) -> UserRet {
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
            let millis = now.as_millis()
                            .min(i64::MAX as u128) as i64;
            stack::poll_at_millis(millis);
        }
        Err(_) => stack::poll(),
    }
    stack::poll_socket_events();
}
