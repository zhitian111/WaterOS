//! `statfs(2)`：bring-up 最小兼容实现。
//! 本模块代码由AI完成

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::ProcessCredentials;
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsNodeType};

use super::path_at::{resolve_path_at, resolve_symlinks, AT_FDCWD};
use vfs::api::FinalSymlink;
use crate::user_copy::{copy_to_user_struct, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;

// 本变量代码由AI完成
const EXT4_SUPER_MAGIC: isize = 0xEF53;
const STATFS_BLOCK_SIZE: isize = 4096;
const STATFS_TOTAL_BLOCKS: isize = 1024 * 1024;
const STATFS_FREE_BLOCKS: isize = 512 * 1024;
const STATFS_TOTAL_FILES: isize = 1024 * 1024;
const STATFS_FREE_FILES: isize = 512 * 1024;
const STATFS_MAX_NAME_LEN: isize = 255;

// 本结构代码由AI完成
#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxStatFs {
    f_type: isize,
    f_bsize: isize,
    f_blocks: isize,
    f_bfree: isize,
    f_bavail: isize,
    f_files: isize,
    f_ffree: isize,
    f_fsid: [i32; 2],
    f_namelen: isize,
    f_frsize: isize,
    f_flags: isize,
    f_spare: [isize; 4],
}

const _: () = assert!(core::mem::size_of::<LinuxStatFs>() == 120);

fn make_statfs_for_path(path: Option<&str>) -> LinuxStatFs {
    let f_type = path
        .and_then(vfs::mount_statfs_magic)
        .unwrap_or(EXT4_SUPER_MAGIC);

    LinuxStatFs {
        f_type,
        f_bsize: STATFS_BLOCK_SIZE,
        f_blocks: STATFS_TOTAL_BLOCKS,
        f_bfree: STATFS_FREE_BLOCKS,
        f_bavail: STATFS_FREE_BLOCKS,
        f_files: STATFS_TOTAL_FILES,
        f_ffree: STATFS_FREE_FILES,
        f_fsid: [0; 2],
        f_namelen: STATFS_MAX_NAME_LEN,
        f_frsize: STATFS_BLOCK_SIZE,
        f_flags: 0,
        f_spare: [0; 4],
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_statfs(args: SyscallArgs) -> UserRet {
    let path_ptr = args.arg(0);
    let buf_ptr = args.arg(1);
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_path_at(AT_FDCWD, path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let cred = cred::current_credentials();
    if let Err(e) = check_parent_search(resolved.as_str(), &cred) {
        return UserRet::from_error(e);
    }
    let resolved = match resolve_symlinks(resolved.as_str(), FinalSymlink::Follow) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    match active_impl::backend().metadata(resolved.as_str()) {
        Ok(_) => {}
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    }

    let statfs = make_statfs_for_path(Some(resolved.as_str()));

    match copy_to_user_struct(buf_ptr, &statfs) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
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
pub(crate) fn sys_fstatfs(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let buf_ptr = args.arg(1);
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let backing_path = match vfs::fd::with_current_io(fd, |handle| {
        Ok(handle.backing_path().map(alloc::string::String::from))
    }) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };
    let statfs = make_statfs_for_path(backing_path.as_deref());

    match copy_to_user_struct(buf_ptr, &statfs) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}
