//! `umount2(2)`：卸载辅助卷挂载点；支持 `MNT_DETACH`。
//! 本模块代码由AI完成

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use vfs::api::VfsError;

use crate::sys::fs::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const MNT_DETACH: u32 = 2;

// 本方法代码由AI完成
pub(crate) fn sys_umount2(args: SyscallArgs) -> UserRet {
    // WaterOS 尚未实现 capability namespace；euid 0 对应 Linux CAP_SYS_ADMIN。
    if cred::current_credentials().effective_uid.0 != 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }

    let target_ptr = args.arg(0);
    let flags = args.arg(1) as u32;

    if flags != 0 && flags != MNT_DETACH {
        log::warn!("[syscall] umount2(nr=166) unsupported flags={:#x}", flags);
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if target_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let target = match copy_user_path_cstr(target_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let mount_point = match resolve_path_at(
        crate::sys::fs::path_at::AT_FDCWD,
        target.as_str(),
    ) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let detach = flags == MNT_DETACH;
    match vfs::unmount_at(mount_point.as_str(), detach) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotFound) => UserRet::from_error(ErrNo::EINVAL),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
