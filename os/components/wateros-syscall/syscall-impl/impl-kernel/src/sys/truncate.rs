//! `truncate(2)`：按路径调整普通文件长度。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::sys::path_at::{resolve_path_at, AT_FDCWD};
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

pub(crate) fn sys_truncate(args: SyscallArgs) -> UserRet {
    let path_ptr = args.arg(0);
    let len = args.arg(1) as u64;

    if path_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(AT_FDCWD, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::truncate_absolute(resolved.as_str(), len) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::EISDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
