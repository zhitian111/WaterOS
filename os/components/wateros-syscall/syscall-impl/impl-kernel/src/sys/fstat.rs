//! `fstat(2)`：将已打开文件的元数据写入用户 `stat` 缓冲。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VFS_FIRST_DYNAMIC_FD, VFS_STDERR_FD, VFS_STDIN_FD};

use crate::linux_stat::{fill_linux_stat, fill_linux_statx};
use crate::sys::stat_times;
use crate::sys::path_at::{resolve_final_symlink, resolve_path_at};
use crate::user_copy::{copy_to_user_struct, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;

const AT_EMPTY_PATH: u32 = 0x1000;
const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
const AT_NO_AUTOMOUNT: u32 = 0x800;
const AT_STATX_FORCE_SYNC: u32 = 0x2000;
const AT_STATX_DONT_SYNC: u32 = 0x4000;
const AT_STATX_SYNC_TYPE: u32 = AT_STATX_FORCE_SYNC | AT_STATX_DONT_SYNC;
const STATX_RESERVED: u32 = 0x8000_0000;
const NAME_MAX: usize = 255;

fn reject_long_path_component(path: &str) -> Result<(), ErrNo> {
    if path.split('/').any(|component| component.len() > NAME_MAX) {
        return Err(ErrNo::ENAMETOOLONG);
    }
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn sys_fstat(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let stat_ptr = args.arg(1);
    if stat_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if fd < VFS_FIRST_DYNAMIC_FD && (fd > VFS_STDERR_FD || fd < VFS_STDIN_FD) {
        return UserRet::from_error(ErrNo::EBADF);
    }

    match vfs::fd::with_current_io(fd, |handle| {
        let meta = handle.metadata()?;
        let mut stat = fill_linux_stat(&meta, meta.size);
        stat_times::apply_stat(&meta, &mut stat);
        Ok(stat)
    }) {
        Ok(stat) => match copy_to_user_struct(stat_ptr, &stat) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        },
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_fstatat(args: SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let stat_ptr = args.arg(2);
    let flags = args.arg(3) as u32;
    if stat_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let path = if path_ptr == 0 && (flags & AT_EMPTY_PATH) != 0 {
        alloc::string::String::new()
    } else {
        match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
            Ok(p) => p,
            Err(e) => return UserRet::from_error(e),
        }
    };

    let stat = if path.is_empty() && (flags & AT_EMPTY_PATH) != 0 && dirfd >= 0 {
        match vfs::fd::with_current_io(dirfd as usize, |handle| {
            let meta = handle.metadata()?;
            let mut stat = fill_linux_stat(&meta, meta.size);
            stat_times::apply_stat(&meta, &mut stat);
            Ok(stat)
        }) {
            Ok(stat) => stat,
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    } else {
        let resolved = match resolve_path_at(dirfd, path.as_str()) {
            Ok(path) => path,
            Err(e) => return UserRet::from_error(e),
        };
        match active_impl::backend().metadata(resolved.as_str()) {
            Ok(meta) => {
                let mut stat = fill_linux_stat(&meta, meta.size);
                stat_times::apply_stat(&meta, &mut stat);
                stat
            }
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    };

    match copy_to_user_struct(stat_ptr, &stat) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_statx(args: SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let flags = args.arg(2) as u32;
    let mask = args.arg(3) as u32;
    let statx_ptr = args.arg(4);
    if statx_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if flags & !(AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW | AT_NO_AUTOMOUNT | AT_STATX_SYNC_TYPE) != 0
        || flags & AT_STATX_SYNC_TYPE == AT_STATX_SYNC_TYPE
        || mask & STATX_RESERVED != 0
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let path = if path_ptr == 0 && (flags & AT_EMPTY_PATH) != 0 {
        alloc::string::String::new()
    } else {
        match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
            Ok(p) => p,
            Err(e) => return UserRet::from_error(e),
        }
    };
    if let Err(e) = reject_long_path_component(path.as_str()) {
        return UserRet::from_error(e);
    }

    let statx = if path.is_empty() && (flags & AT_EMPTY_PATH) != 0 && dirfd >= 0 {
        match vfs::fd::with_current_io(dirfd as usize, |handle| {
            let meta = handle.metadata()?;
            let mut statx = fill_linux_statx(&meta, meta.size, mask);
            stat_times::apply_statx(&meta, &mut statx);
            Ok(statx)
        }) {
            Ok(statx) => statx,
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    } else {
        let resolved = match resolve_path_at(dirfd, path.as_str()) {
            Ok(path) => path,
            Err(e) => return UserRet::from_error(e),
        };
        let resolved = if flags & AT_SYMLINK_NOFOLLOW != 0 {
            resolved
        } else {
            match resolve_final_symlink(resolved.as_str()) {
                Ok(path) => path,
                Err(e) => return UserRet::from_error(e),
            }
        };
        match active_impl::backend().metadata(resolved.as_str()) {
            Ok(meta) => {
                let mut statx = fill_linux_statx(&meta, meta.size, mask);
                stat_times::apply_statx(&meta, &mut statx);
                statx
            }
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    };

    match copy_to_user_struct(statx_ptr, &statx) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}
