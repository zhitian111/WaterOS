//! `mkdirat(2)`：相对 `AT_FDCWD` 或目录 fd 创建目录。

use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use super::ltp_cgroup_helper::cgroup_regression_loop_fast_exit_if_standalone;
use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

pub(crate) fn sys_mkdirat(args: SyscallArgs) -> UserRet {
    cgroup_regression_loop_fast_exit_if_standalone();

    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let mode = args.arg(2) as u32;

    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::mkdir_absolute(resolved.as_str(), mode) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotAFile) => UserRet::from_error(abi::errno::ErrNo::ENOTDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
