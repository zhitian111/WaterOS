//! `read(2)`：支持 pipe 读端；stdin 暂未接真实输入。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::vfs_util::vfs_error_to_errno;

pub(crate) fn sys_read(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1) as *mut u8;
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr.is_null() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if len > 4 * 1024 * 1024 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let buf = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
    match vfs::fd::with_current_io(fd, |handle| handle.read(buf)) {
        Ok(n) => UserRet::from_success(n),
        Err(err) => UserRet::from_error(vfs_error_to_errno(err)),
    }
}
