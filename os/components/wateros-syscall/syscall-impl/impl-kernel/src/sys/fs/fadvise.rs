//! `fadvise64(2)` compatibility for file access-pattern hints.

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::{VfsError, VfsNodeType};

const POSIX_FADV_NORMAL : usize = 0;
const POSIX_FADV_RANDOM : usize = 1;
const POSIX_FADV_SEQUENTIAL : usize = 2;
const POSIX_FADV_WILLNEED : usize = 3;
const POSIX_FADV_DONTNEED : usize = 4;
const POSIX_FADV_NOREUSE : usize = 5;

pub(crate) fn sys_fadvise64(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let offset = args.arg(1) as isize;
    let length = args.arg(2) as isize;
    let advice = args.arg(3);

    if offset < 0 || length < 0 || !valid_advice(advice) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    match vfs::fd::is_path_only_fd(fd) {
        Ok(true) => return UserRet::from_error(ErrNo::EBADF),
        Ok(false) => {}
        Err(VfsError::BadFd) => return UserRet::from_error(ErrNo::EBADF),
        Err(_) => return UserRet::from_error(ErrNo::EBADF),
    }
    match vfs::fd::with_current_io(fd, |handle| handle.metadata()) {
        Ok(meta) if meta.node_type == VfsNodeType::File => UserRet::from_success(0),
        Ok(_) => UserRet::from_error(ErrNo::ESPIPE),
        Err(VfsError::BadFd) => UserRet::from_error(ErrNo::EBADF),
        Err(_) => UserRet::from_error(ErrNo::ESPIPE),
    }
}

#[inline]
fn valid_advice(advice : usize) -> bool {
    matches!(advice,
             POSIX_FADV_NORMAL |
             POSIX_FADV_RANDOM |
             POSIX_FADV_SEQUENTIAL |
             POSIX_FADV_WILLNEED |
             POSIX_FADV_DONTNEED |
             POSIX_FADV_NOREUSE)
}
