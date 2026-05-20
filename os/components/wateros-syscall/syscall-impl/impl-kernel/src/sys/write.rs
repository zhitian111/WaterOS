//! `write(2)`：fd 1/2 走控制台；pipe 写端走 IPC。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::copy_from_user;
use crate::vfs_util::vfs_error_to_errno;

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
    match vfs::fd::with_current_io(fd, |handle| handle.write(&kbuf)) {
        Ok(n) => UserRet::from_success(n),
        Err(err) => UserRet::from_error(vfs_error_to_errno(err)),
    }
}
