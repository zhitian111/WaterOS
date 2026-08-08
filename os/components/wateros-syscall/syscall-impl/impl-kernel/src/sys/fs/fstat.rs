//! `fstat(2)`：将已打开文件的元数据写入用户 `stat` 缓冲。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::ProcessCredentials;
use vfs::active_impl;
use vfs::api::{
    SingleRootReadView, VfsError, VfsMetadata, VfsNodeType, VFS_FIRST_DYNAMIC_FD, VFS_STDERR_FD,
    VFS_STDIN_FD,
};

use crate::linux_stat::{fill_linux_stat, fill_linux_statx};
use crate::sys::stat_times;
use super::path_at::{resolve_path_at, resolve_symlinks};
use vfs::api::FinalSymlink;
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

fn check_stat_parent_search(path: &str, cred: &ProcessCredentials) -> Result<(), ErrNo> {
    if cred.effective_uid.0 == 0 {
        return Ok(());
    }
    let parts: alloc::vec::Vec<&str> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() <= 1 {
        return Ok(());
    }
    let mut current = alloc::string::String::from("/");
    for part in &parts[..parts.len() - 1] {
        if current != "/" {
            current.push('/');
        }
        current.push_str(part);
        match active_impl::backend().metadata(current.as_str()) {
            Ok(meta) if meta.node_type == VfsNodeType::Directory => {
                if meta.mode & 0o111 == 0 {
                    return Err(ErrNo::EACCES);
                }
            }
            Ok(_) => return Err(ErrNo::ENOTDIR),
            Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
            Err(e) => return Err(vfs_error_to_errno(e)),
        }
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
    if flags & !(AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW | AT_NO_AUTOMOUNT) != 0 {
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
        let final_symlink = if flags & AT_SYMLINK_NOFOLLOW != 0 {
            FinalSymlink::NoFollow
        } else {
            FinalSymlink::Follow
        };
        let resolved = match resolve_symlinks(resolved.as_str(), final_symlink) {
            Ok(path) => path,
            Err(e) => return UserRet::from_error(e),
        };
        let cred = cred::current_credentials();
        if let Err(e) = check_stat_parent_search(resolved.as_str(), &cred) {
            return UserRet::from_error(e);
        }
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
            match resolve_symlinks(resolved.as_str(), FinalSymlink::NoFollow) {
                Ok(path) => path,
                Err(error) => return UserRet::from_error(error),
            }
        } else {
            match resolve_symlinks(resolved.as_str(), FinalSymlink::Follow) {
                Ok(path) => path,
                Err(e) => return UserRet::from_error(e),
            }
        };
        let cred = cred::current_credentials();
        if let Err(e) = check_stat_parent_search(resolved.as_str(), &cred) {
            return UserRet::from_error(e);
        }
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
