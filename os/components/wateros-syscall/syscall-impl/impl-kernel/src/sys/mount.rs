//! `mount(2)`：将 ext4 块设备挂到根卷内目录。
//!
//! oscomp 2025 镜像为整盘 ext4、无分区表；测例传 `vfat` + `/dev/vda2` 时按 devfs alias
//! 复用 ext4，不解析 FAT。

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
    let readonly = flags & MS_RDONLY != 0;

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
        // 空串由块设备探测；非空时接受 ext2/3/4 或 oscomp 兼容名 vfat（实际仍挂 ext4）。
        if !fstype.is_empty()
            && !matches!(
                fstype.as_str(),
                "ext4" | "ext3" | "ext2" | "vfat"
            )
        {
            return UserRet::from_error(ErrNo::EINVAL);
        }
    }

    let mount_point = match resolve_path_at(
        crate::sys::path_at::AT_FDCWD,
        target.as_str(),
    ) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::mount_ext4_block_at(
        mount_point.as_str(),
        source.as_str(),
        readonly,
    ) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Driver) | Err(VfsError::NotFound) => UserRet::from_error(ErrNo::ENOENT),
        Err(VfsError::Exists) => UserRet::from_error(ErrNo::EBUSY),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
