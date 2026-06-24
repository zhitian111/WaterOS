//! `symlinkat(2)`：相对目录 fd 创建符号链接。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const PATH_MAX: usize = 4096;

pub(crate) fn sys_symlinkat(args: SyscallArgs) -> UserRet {
    let target_ptr = args.arg(0);
    let dirfd = args.arg(1) as isize;
    let linkpath_ptr = args.arg(2);

    let target = match copy_user_path_cstr(target_ptr, PATH_MAX) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let linkpath = match copy_user_path_cstr(linkpath_ptr, PATH_MAX) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(dirfd, linkpath.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::symlink_absolute(target.as_str(), resolved.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Exists) => UserRet::from_error(ErrNo::EEXIST),
        Err(VfsError::NotFound) => UserRet::from_error(ErrNo::ENOENT),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::ENOTDIR),
        Err(VfsError::ReadOnlyFs) => UserRet::from_error(ErrNo::EROFS),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
