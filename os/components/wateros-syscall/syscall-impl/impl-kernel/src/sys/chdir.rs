//! `chdir(2)`：切换当前任务工作目录。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

// 本方法代码由AI完成
pub(crate) fn sys_chdir(args: SyscallArgs) -> UserRet {
    let path_ptr = args.arg(0);
    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::cwd::chdir_current(path.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::ENOTDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
