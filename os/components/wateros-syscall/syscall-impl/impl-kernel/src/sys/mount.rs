//! `mount(2)`：将 ext4 块设备挂到根卷内空目录。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const MS_RDONLY: u64 = 1;

pub(crate) fn sys_mount(args: SyscallArgs) -> UserRet {
    let source_ptr = args.arg(0);
    let target_ptr = args.arg(1);
    let fstype_ptr = args.arg(2);
    let flags = args.arg(3) as u64;

    if source_ptr == 0 || target_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if flags & MS_RDONLY != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let source = match copy_user_path_cstr(source_ptr, 256) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };
    let target = match copy_user_path_cstr(target_ptr, 256) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    if fstype_ptr != 0 {
        let fstype = match copy_user_path_cstr(fstype_ptr, 32) {
            Ok(s) => s,
            Err(e) => return UserRet::from_error(e),
        };
        if fstype != "ext4" {
            return UserRet::from_error(ErrNo::EINVAL);
        }
    }

    let mount_point = match resolve_path_at(crate::sys::path_at::AT_FDCWD, target.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::mount_ext4_block_at(mount_point.as_str(), source.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Driver) | Err(VfsError::NotFound) => UserRet::from_error(ErrNo::ENOENT),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
