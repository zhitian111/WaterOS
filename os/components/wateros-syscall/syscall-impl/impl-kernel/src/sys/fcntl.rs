//! `fcntl(2)` — 文件描述符控制。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::vfs_util::vfs_error_to_errno;

const F_DUPFD: usize = 0;
const F_GETFD: usize = 1;
const F_SETFD: usize = 2;
const F_GETFL: usize = 3;
const F_SETFL: usize = 4;

const FD_CLOEXEC: usize = 1;

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
        _ => Err(ErrNo::ENOSYS),
    };

    match result {
        Ok(n) => UserRet::from_success(n),
        Err(e) => UserRet::from_error(e),
    }
}

fn fcntl_dupfd(fd: usize, minfd: usize) -> Result<usize, ErrNo> {
    vfs::fd::dup_fd(fd, minfd).map_err(vfs_error_to_errno)
}

fn fcntl_getfd(fd: usize) -> Result<usize, ErrNo> {
    vfs::fd::get_fd_flags(fd).map_err(vfs_error_to_errno)
}

fn fcntl_setfd(fd: usize, arg: usize) -> Result<usize, ErrNo> {
    if arg & !FD_CLOEXEC != 0 {
        return Err(ErrNo::EINVAL);
    }
    vfs::fd::set_fd_flags(fd, arg)
        .map_err(vfs_error_to_errno)?;
    Ok(0)
}

fn fcntl_getfl(_fd: usize) -> Result<usize, ErrNo> {
    Ok(0x2)
}

fn fcntl_setfl(_fd: usize, _arg: usize) -> Result<usize, ErrNo> {
    Ok(0)
}
