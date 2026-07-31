//! `truncate(2)`：按路径调整普通文件长度。
//! 本模块代码由AI完成

extern crate alloc;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsNodeType};

use super::path_at::{resolve_path_at, resolve_symlinks, AT_FDCWD};
use vfs::api::FinalSymlink;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const RLIMIT_FSIZE: usize = 1;

// 本方法代码由AI完成
pub(crate) fn sys_truncate(args: SyscallArgs) -> UserRet {
    let path_ptr = args.arg(0);
    let raw_len = args.arg(1);

    if path_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if (raw_len as isize) < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let len = raw_len as u64;
    if exceeds_fsize_rlimit(len) {
        return UserRet::from_error(ErrNo::EFBIG);
    }

    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(AT_FDCWD, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_symlinks(resolved.as_str(), FinalSymlink::Follow) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let cred = cred::current_credentials();
    if let Err(e) = check_parent_search(resolved.as_str(), &cred) {
        return UserRet::from_error(e);
    }
    match active_impl::backend().metadata(resolved.as_str()) {
        Ok(meta) => {
            if meta.node_type == VfsNodeType::Directory {
                return UserRet::from_error(ErrNo::EISDIR);
            }
            if meta.node_type != VfsNodeType::File {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            if !can_write_file(&meta, &cred) {
                return UserRet::from_error(ErrNo::EACCES);
            }
        }
        Err(VfsError::NotAFile) => return UserRet::from_error(ErrNo::ENOTDIR),
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    }

    match vfs::truncate_absolute(resolved.as_str(), len) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::EISDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn exceeds_fsize_rlimit(len: u64) -> bool {
    let Some(pid) = task::current_process_task_snapshot().map(|snapshot| snapshot.pid) else {
        return false;
    };
    let Some(limit) = task::process_resource_limit(pid, RLIMIT_FSIZE) else {
        return false;
    };
    len > limit.cur
}

fn can_write_file(meta: &vfs::api::VfsMetadata, cred: &ProcessCredentials) -> bool {
    if cred.effective_uid.0 == 0 {
        return true;
    }
    let mode = u32::from(meta.mode);
    if cred.effective_uid.0 == meta.uid {
        return mode & 0o200 != 0;
    }
    if cred_has_group(cred, Gid(meta.gid)) {
        return mode & 0o020 != 0;
    }
    mode & 0o002 != 0
}

fn cred_has_group(cred: &ProcessCredentials, gid: Gid) -> bool {
    cred.effective_gid == gid
        || cred
            .supplementary_groups
            .iter()
            .take(cred.supplementary_group_len)
            .any(|group| *group == gid)
}

fn check_parent_search(path: &str, cred: &ProcessCredentials) -> Result<(), ErrNo> {
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
        let meta = active_impl::backend()
            .metadata(current.as_str())
            .map_err(vfs_error_to_errno)?;
        if meta.node_type != VfsNodeType::Directory {
            return Err(ErrNo::ENOTDIR);
        }
        let mode = u32::from(meta.mode);
        let executable = if cred.effective_uid.0 == meta.uid {
            mode & 0o100 != 0
        } else if cred_has_group(cred, Gid(meta.gid)) {
            mode & 0o010 != 0
        } else {
            mode & 0o001 != 0
        };
        if !executable {
            return Err(ErrNo::EACCES);
        }
    }
    Ok(())
}
pub(crate) fn sys_ftruncate(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let len = args.arg(1);

    if (len as isize) < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let result = loop {
        let result = vfs::fd::with_current_io(fd, |handle| {
            const O_ACCMODE: u32 = 3;
            const O_RDONLY: u32 = 0;
            if handle.open_accmode() & O_ACCMODE == O_RDONLY {
                return Err(vfs::api::VfsError::Unsupported);
            }
            handle.truncate(len as u64)
        });
        if result != Err(vfs::api::VfsError::Busy) {
            break result;
        }
        task::yield_now();
    };
    match result {
        Ok(()) => UserRet::from_success(0),
        Err(vfs::api::VfsError::Unsupported) => UserRet::from_error(ErrNo::EINVAL),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
