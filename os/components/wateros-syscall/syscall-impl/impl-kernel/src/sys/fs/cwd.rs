//! 当前工作目录操作：`chdir(2)`、`getcwd(2)`。

use alloc::string::String;
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata, VfsNodeType};

use super::path_at::{resolve_path_at, resolve_symlinks, AT_FDCWD};
use vfs::api::FinalSymlink;
use crate::user_copy::{copy_to_user, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;

fn can_search_directory(meta: &VfsMetadata, cred: &ProcessCredentials) -> bool {
    if cred.effective_uid.0 == 0 {
        return true;
    }
    let mode = u32::from(meta.mode);
    let required = if cred.effective_uid.0 == meta.uid {
        0o100
    } else if cred_has_group(cred, Gid(meta.gid)) {
        0o010
    } else {
        0o001
    };
    mode & required != 0
}

fn cred_has_group(cred: &ProcessCredentials, gid: Gid) -> bool {
    cred.effective_gid == gid || cred.supplementary_groups.iter().any(|g| *g == gid)
}

// 本方法代码由AI完成
pub(crate) fn sys_chdir(args: SyscallArgs) -> UserRet {
    let path_ptr = args.arg(0);
    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(AT_FDCWD, path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_symlinks(resolved.as_str(), FinalSymlink::Follow) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let meta = match active_impl::backend().metadata(resolved.as_str()) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => meta,
        Ok(_) => return UserRet::from_error(ErrNo::ENOTDIR),
        Err(VfsError::NotFound) => return UserRet::from_error(ErrNo::ENOENT),
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };
    let cred = cred::current_credentials();
    if !can_search_directory(&meta, &cred) {
        return UserRet::from_error(ErrNo::EACCES);
    }
    let task_id = match vfs::fd::current_task_id() {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };

    match vfs::cwd::set_task_cwd(task_id, resolved.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::ENOTDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

pub(crate) fn sys_chroot(args : SyscallArgs) -> UserRet {
    if cred::current_credentials().effective_uid.0 != 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }
    let path = match copy_user_path_cstr(args.arg(0), crate::user_copy::USER_PATH_MAX) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    let resolved = match resolve_path_at(AT_FDCWD, path.as_str()) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    let resolved = match resolve_symlinks(resolved.as_str(), FinalSymlink::Follow) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    match vfs::cwd::chroot_current_resolved(resolved.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotAFile | VfsError::NotDirectory) => {
            UserRet::from_error(ErrNo::ENOTDIR)
        }
        Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
    }
}

pub(crate) fn sys_fchdir(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let path = match vfs::fd::with_current_io(fd, |handle| {
        handle.directory_path()
              .map(String::from)
              .ok_or(VfsError::NotDirectory)
    }) {
        Ok(path) => path,
        Err(VfsError::BadFd) => return UserRet::from_error(ErrNo::EBADF),
        Err(VfsError::NotDirectory) | Err(VfsError::NotAFile) => {
            return UserRet::from_error(ErrNo::ENOTDIR);
        }
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    let meta = match active_impl::backend().metadata(path.as_str()) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => meta,
        Ok(_) => return UserRet::from_error(ErrNo::ENOTDIR),
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    if !can_search_directory(&meta, &cred::current_credentials()) {
        return UserRet::from_error(ErrNo::EACCES);
    }
    let task_id = match vfs::fd::current_task_id() {
        Ok(task_id) => task_id,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    match vfs::cwd::set_task_cwd(task_id, path.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotAFile) | Err(VfsError::NotDirectory) => {
            UserRet::from_error(ErrNo::ENOTDIR)
        }
        Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_getcwd(args: SyscallArgs) -> UserRet {
    let buf_ptr = args.arg(0);
    let size = args.arg(1);

    let mut kernel_buf = [0u8; 4096];
    let written = match vfs::cwd::write_cwd_to_buf(&mut kernel_buf) {
        Ok(n) => n,
        Err(VfsError::NoTask) => return UserRet::from_error(ErrNo::ESRCH),
        Err(_) => return UserRet::from_error(ErrNo::EINVAL),
    };

    if size < written {
        return UserRet::from_error(ErrNo::ERANGE);
    }
    // Linux 先判断调用方给出的容量。即使 buf 是坏指针，只要容量不足也应
    // 返回 ERANGE；容量足够后才触碰用户地址并可能返回 EFAULT。
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    match copy_to_user(buf_ptr, &kernel_buf[..written]) {
        // The getcwd syscall ABI returns the byte count including the trailing NUL.
        // The libc wrapper converts that result into the caller's buffer pointer.
        Ok(n) if n == written => UserRet::from_success(written),
        Ok(_) => UserRet::from_error(ErrNo::EFAULT),
        Err(e) => UserRet::from_error(e),
    }
}
