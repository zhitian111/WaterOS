//! `mount(2)`：块设备 ext4、tmpfs、procfs 挂载与重载只读。

use alloc::string::String;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const MS_RDONLY: u64 = 1;
const MS_REMOUNT: u64 = 0x800;

pub(crate) fn sys_mount(args: SyscallArgs) -> UserRet {
    let source_ptr = args.arg(0);
    let target_ptr = args.arg(1);
    let fstype_ptr = args.arg(2);
    let flags = args.arg(3) as u64;

    if target_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let target = match copy_user_path_cstr(target_ptr, 256) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let mount_point = match resolve_path_at(crate::sys::path_at::AT_FDCWD, target.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let fstype = if fstype_ptr != 0 {
        match copy_user_path_cstr(fstype_ptr, 32) {
            Ok(s) => s,
            Err(e) => return UserRet::from_error(e),
        }
    } else {
        String::new()
    };

    if flags & MS_REMOUNT != 0 {
        if flags & MS_RDONLY == 0 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        return match vfs::remount_readonly_at(mount_point.as_str()) {
            Ok(()) => UserRet::from_success(0),
            Err(VfsError::NotFound) => UserRet::from_error(ErrNo::EINVAL),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    if fstype == "proc" {
        if vfs::is_proc_mounted_at(mount_point.as_str()) {
            return UserRet::from_error(ErrNo::EBUSY);
        }
        if let Err(e) = vfs::ensure_proc_mount_point() {
            return UserRet::from_error(vfs_error_to_errno(e));
        }
        return match vfs::mount_procfs_at(mount_point.as_str()) {
            Ok(()) => UserRet::from_success(0),
            Err(VfsError::Exists) => UserRet::from_error(ErrNo::EBUSY),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    if fstype == "tmpfs" {
        if source_ptr != 0 {
            let source = match copy_user_path_cstr(source_ptr, 256) {
                Ok(s) => s,
                Err(e) => return UserRet::from_error(e),
            };
            if !source.is_empty() {
                return UserRet::from_error(ErrNo::EINVAL);
            }
        }
        if flags & MS_RDONLY != 0 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        return match vfs::mount_tmpfs_at(mount_point.as_str()) {
            Ok(()) => UserRet::from_success(0),
            Err(VfsError::Exists) => UserRet::from_error(ErrNo::EBUSY),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    if source_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let source = match copy_user_path_cstr(source_ptr, 256) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let readonly = flags & MS_RDONLY != 0;

    if !fstype.is_empty()
        && !matches!(fstype.as_str(), "ext4" | "ext3" | "ext2" | "vfat")
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    match vfs::mount_ext4_block_at(mount_point.as_str(), source.as_str(), readonly) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Driver) | Err(VfsError::NotFound) => UserRet::from_error(ErrNo::ENOENT),
        Err(VfsError::Exists) => UserRet::from_error(ErrNo::EBUSY),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
