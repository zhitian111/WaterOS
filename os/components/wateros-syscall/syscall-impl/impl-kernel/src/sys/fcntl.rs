//! `fcntl(2)` — 文件描述符控制。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::socket_fd;
use crate::vfs_util::vfs_error_to_errno;

const F_DUPFD: usize = 0;
const F_GETFD: usize = 1;
const F_SETFD: usize = 2;
const F_GETFL: usize = 3;
const F_SETFL: usize = 4;
const F_DUPFD_CLOEXEC: usize = 1030;

const FD_CLOEXEC: usize = 1;
const O_ACCMODE_RDWR: usize = 0x2;
const O_NONBLOCK: usize = 0o0004000;

pub(crate) fn sys_fcntl(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let cmd = args.arg(1);
    let arg = args.arg(2);

    let result = match cmd {
        F_DUPFD => fcntl_dupfd(fd, arg),
        F_GETFD => fcntl_getfd(fd),
        F_SETFD => fcntl_setfd(fd, arg),
        F_GETFL => fcntl_getfl(fd),
        F_SETFL => fcntl_setfl(fd, arg),
        F_DUPFD_CLOEXEC => fcntl_dupfd_cloexec(fd, arg),
        _ => Err(ErrNo::ENOSYS),
    };

    match result {
        Ok(n) => UserRet::from_success(n),
        Err(e) => UserRet::from_error(e),
    }
}

fn fcntl_dupfd(fd: usize, minfd: usize) -> Result<usize, ErrNo> {
    let socket = socket_fd::lookup(fd);
    let status_flags = socket_fd::status_flags(fd).unwrap_or(0);
    let newfd = vfs::fd::dup_fd(fd, minfd).map_err(vfs_error_to_errno)?;
    if let Some(socket) = socket {
        socket_fd::register_with_flags(newfd, socket, status_flags);
    }
    Ok(newfd)
}

fn fcntl_dupfd_cloexec(fd: usize, minfd: usize) -> Result<usize, ErrNo> {
    let socket = socket_fd::lookup(fd);
    let status_flags = socket_fd::status_flags(fd).unwrap_or(0);
    let newfd = vfs::fd::dup_fd(fd, minfd).map_err(vfs_error_to_errno)?;
    vfs::fd::set_fd_flags(newfd, FD_CLOEXEC).map_err(vfs_error_to_errno)?;
    if let Some(socket) = socket {
        socket_fd::register_with_flags(newfd, socket, status_flags);
    }
    Ok(newfd)
}

fn fcntl_getfd(fd: usize) -> Result<usize, ErrNo> {
    vfs::fd::get_fd_flags(fd).map_err(vfs_error_to_errno)
}

fn fcntl_setfd(fd: usize, arg: usize) -> Result<usize, ErrNo> {
    if arg & !FD_CLOEXEC != 0 {
        return Err(ErrNo::EINVAL);
    }
    vfs::fd::set_fd_flags(fd, arg).map_err(vfs_error_to_errno)?;
    Ok(0)
}

fn fcntl_getfl(fd: usize) -> Result<usize, ErrNo> {
    if let Some(flags) = socket_fd::status_flags(fd) {
        return Ok(O_ACCMODE_RDWR | (flags & O_NONBLOCK));
    }
    Ok(O_ACCMODE_RDWR)
}

fn fcntl_setfl(fd: usize, arg: usize) -> Result<usize, ErrNo> {
    if socket_fd::lookup(fd).is_some() {
        let flags = arg & O_NONBLOCK;
        socket_fd::set_status_flags(fd, flags).ok_or(ErrNo::EBADF)?;
        return Ok(0);
    }
    Ok(0)
}
