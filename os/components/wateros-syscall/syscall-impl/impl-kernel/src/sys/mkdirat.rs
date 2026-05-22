//! `mkdirat(2)`：相对当前任务 cwd 创建目录（首期仅 `AT_FDCWD`）。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

/// Linux `AT_FDCWD`。
const AT_FDCWD: isize = -100;

pub(crate) fn sys_mkdirat(args: SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let mode = args.arg(2) as u32;

    if dirfd != AT_FDCWD {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::mkdir_at_current(path.as_str(), mode) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::ENOTDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
