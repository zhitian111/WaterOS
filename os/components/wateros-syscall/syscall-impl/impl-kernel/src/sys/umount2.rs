//! `umount2(2)`：卸载辅助卷挂载点。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

pub(crate) fn sys_umount2(args: SyscallArgs) -> UserRet {
    let target_ptr = args.arg(0);
    let _flags = args.arg(1) as u32;

    if target_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let target = match copy_user_path_cstr(target_ptr, 256) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let mount_point = match resolve_path_at(
        crate::sys::path_at::AT_FDCWD,
        target.as_str(),
    ) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::unmount_at(mount_point.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotFound) => UserRet::from_error(ErrNo::EINVAL),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
