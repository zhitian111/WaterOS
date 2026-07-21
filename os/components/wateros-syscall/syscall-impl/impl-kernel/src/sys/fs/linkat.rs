//! `linkat(2)`：相对目录 fd 创建硬链接。
//! 本模块代码由AI完成

extern crate alloc;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata, VfsNodeType};

use super::path_at::{resolve_final_symlink, resolve_path_at};
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const AT_SYMLINK_FOLLOW : u32 = 0x400;

// 本方法代码由AI完成
pub(crate) fn sys_linkat(args : SyscallArgs) -> UserRet {
    let old_dirfd = args.arg(0) as isize;
    let old_path_ptr = args.arg(1);
    let new_dirfd = args.arg(2) as isize;
    let new_path_ptr = args.arg(3);
    let flags = args.arg(4) as u32;

    if flags & !AT_SYMLINK_FOLLOW != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let old_path = match copy_user_path_cstr(old_path_ptr,
                                             crate::user_copy::USER_PATH_MAX)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let new_path = match copy_user_path_cstr(new_path_ptr,
                                             crate::user_copy::USER_PATH_MAX)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    let old_resolved = match resolve_path_at(old_dirfd, old_path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let old_resolved = if flags & AT_SYMLINK_FOLLOW != 0 {
        match resolve_final_symlink(old_resolved.as_str()) {
            Ok(path) => path,
            Err(e) => return UserRet::from_error(e),
        }
    } else {
        old_resolved
    };
    let new_resolved = match resolve_path_at(new_dirfd, new_path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    if let Err(e) = reject_non_directory_prefix(old_resolved.as_str()) {
        return UserRet::from_error(e);
    }
    if let Err(e) = reject_non_directory_prefix(new_resolved.as_str()) {
        return UserRet::from_error(e);
    }

    match active_impl::backend().metadata(old_resolved.as_str()) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => {
            return UserRet::from_error(ErrNo::EPERM);
        }
        Ok(_) => {}
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    }

    let cred = cred::current_credentials();
    if let Err(e) = check_parent_create(new_resolved.as_str(), &cred) {
        return UserRet::from_error(e);
    }

    if vfs::mount_statfs_magic(old_resolved.as_str()) !=
       vfs::mount_statfs_magic(new_resolved.as_str())
    {
        return UserRet::from_error(ErrNo::EXDEV);
    }

    match vfs::hardlink_absolute(old_resolved.as_str(),
                                 new_resolved.as_str())
    {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Exists) => UserRet::from_error(ErrNo::EEXIST),
        Err(VfsError::ReadOnlyFs) => UserRet::from_error(ErrNo::EROFS),
        Err(VfsError::Unsupported) => UserRet::from_error(ErrNo::EXDEV),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::EPERM),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn reject_non_directory_prefix(path : &str) -> Result<(), ErrNo> {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Ok(());
    }
    let mut current = alloc::string::String::from("/");
    let mut parts = trimmed.split('/')
                           .peekable();
    while let Some(component) = parts.next() {
        if parts.peek()
                .is_none()
        {
            break;
        }
        if current != "/" {
            current.push('/');
        }
        current.push_str(component);
        match active_impl::backend().metadata(current.as_str()) {
            Ok(meta) if meta.node_type != VfsNodeType::Directory => return Err(ErrNo::ENOTDIR),
            Ok(_) => {}
            Err(VfsError::NotFound) => return Ok(()),
            Err(e) => return Err(vfs_error_to_errno(e)),
        }
    }
    Ok(())
}

fn check_parent_create(path : &str, cred : &ProcessCredentials) -> Result<(), ErrNo> {
    let parent = path.rsplit_once('/')
                     .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
                     .unwrap_or("/");
    let meta = match active_impl::backend().metadata(parent) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => meta,
        Ok(_) => return Err(ErrNo::ENOTDIR),
        Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
        Err(e) => return Err(vfs_error_to_errno(e)),
    };
    if cred.effective_uid.0 == 0 || can_write_search_directory(&meta, cred) {
        Ok(())
    } else {
        Err(ErrNo::EACCES)
    }
}

fn can_write_search_directory(meta : &VfsMetadata, cred : &ProcessCredentials) -> bool {
    let mode = u32::from(meta.mode);
    let required = if cred.effective_uid.0 == meta.uid {
        0o300
    } else if cred_has_group(cred, Gid(meta.gid)) {
        0o030
    } else {
        0o003
    };
    mode & required == required
}

fn cred_has_group(cred : &ProcessCredentials, gid : Gid) -> bool {
    cred.effective_gid == gid ||
    cred.supplementary_groups
        .iter()
        .take(cred.supplementary_group_len)
        .any(|group| *group == gid)
}
