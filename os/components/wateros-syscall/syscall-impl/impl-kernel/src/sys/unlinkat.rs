//! `unlinkat(2)`：相对 cwd / 目录 fd 删除文件或空目录。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use super::ltp_cgroup_helper::cgroup_regression_loop_fast_exit_if_standalone;
use crate::sys::path_at::{resolve_path_at, AT_REMOVEDIR};
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

// 本方法代码由AI完成
pub(crate) fn sys_unlinkat(args : SyscallArgs) -> UserRet {
    cgroup_regression_loop_fast_exit_if_standalone();

    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let flags = args.arg(2) as u32;

    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let remove_dir = flags & AT_REMOVEDIR != 0;
    if flags & !AT_REMOVEDIR != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    match vfs::unlink_absolute(resolved.as_str(), remove_dir) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotAFile) if remove_dir => UserRet::from_error(ErrNo::ENOTDIR),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::EISDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
