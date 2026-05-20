//! `getcwd(2)`：将当前工作目录写入用户缓冲区。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::user_copy::copy_to_user;

pub(crate) fn sys_getcwd(args: SyscallArgs) -> UserRet {
    let buf_ptr = args.arg(0);
    let size = args.arg(1);

    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if size == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let mut kernel_buf = [0u8; 256];
    let written = match vfs::cwd::write_cwd_to_buf(&mut kernel_buf) {
        Ok(n) => n,
        Err(VfsError::NoTask) => return UserRet::from_error(ErrNo::ESRCH),
        Err(_) => return UserRet::from_error(ErrNo::EINVAL),
    };

    if size < written {
        return UserRet::from_error(ErrNo::ERANGE);
    }

    match copy_to_user(buf_ptr, &kernel_buf[..written]) {
        Ok(n) if n == written => UserRet::from_success(buf_ptr),
        Ok(_) => UserRet::from_error(ErrNo::EFAULT),
        Err(e) => UserRet::from_error(e),
    }
}
