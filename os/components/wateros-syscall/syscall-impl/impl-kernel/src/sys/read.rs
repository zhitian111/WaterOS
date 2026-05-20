//! `read(2)`：支持 pipe 读端；stdin 暂未接真实输入。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::copy_to_user;
use crate::vfs_util::vfs_error_to_errno;

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
    let mut kbuf = Vec::with_capacity(len);
    kbuf.resize(len, 0);
    let n = match vfs::fd::with_current_io(fd, |handle| handle.read(&mut kbuf)) {
        Ok(n) => n,
        Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
    };
    if n == 0 {
        return UserRet::from_success(0);
    }
    match copy_to_user(ptr, &kbuf[..n]) {
        Ok(written) if written == n => UserRet::from_success(n),
        _ => UserRet::from_error(ErrNo::EFAULT),
    }
}
