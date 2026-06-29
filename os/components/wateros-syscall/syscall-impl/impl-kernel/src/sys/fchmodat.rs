//! `fchmodat(2)`：相对目录修改路径权限（首期支持 `resolve_path_at` 路径解析）。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

// 本方法代码由AI完成
pub(crate) fn sys_fchmodat(args: SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let mode = (args.arg(2) as u32) & 0o7777;
    let flags = args.arg(3) as u32;

    if flags != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::chmod_absolute(resolved.as_str(), mode) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Unsupported) => UserRet::from_error(ErrNo::EPERM),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
