//! `renameat2(2)`：bring-up 同父目录 rename（文件与目录）；非 journal 原子语义。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::format;
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata, VfsNodeType};

use super::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const RENAME_NOREPLACE: u32 = 1;
const RENAME_EXCHANGE: u32 = 2;
const RENAME_WHITEOUT: u32 = 4;
const NAME_MAX: usize = 255;

// 本方法代码由AI完成
pub(crate) fn sys_renameat2(args: SyscallArgs) -> UserRet {
    let old_dirfd = args.arg(0) as isize;
    let old_path_ptr = args.arg(1);
    let new_dirfd = args.arg(2) as isize;
    let new_path_ptr = args.arg(3);
    let flags = args.arg(4) as u32;

    rename_at_impl(old_dirfd, old_path_ptr, new_dirfd, new_path_ptr, flags)
}

// 本方法代码由AI完成
pub(crate) fn sys_renameat(args: SyscallArgs) -> UserRet {
    let old_dirfd = args.arg(0) as isize;
    let old_path_ptr = args.arg(1);
    let new_dirfd = args.arg(2) as isize;
    let new_path_ptr = args.arg(3);

    rename_at_impl(old_dirfd, old_path_ptr, new_dirfd, new_path_ptr, 0)
}

fn rename_at_impl(
    old_dirfd: isize,
    old_path_ptr: usize,
    new_dirfd: isize,
    new_path_ptr: usize,
    flags: u32,
) -> UserRet {
    if flags & !(RENAME_NOREPLACE | RENAME_EXCHANGE | RENAME_WHITEOUT) != 0 {
        log::warn!(
            "[syscall] renameat2(nr=276) unsupported flags={:#x}",
            flags,
        );
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & RENAME_EXCHANGE != 0 && flags & (RENAME_NOREPLACE | RENAME_WHITEOUT) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & RENAME_WHITEOUT != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let old_path = match copy_user_path_cstr(old_path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let new_path = match copy_user_path_cstr(new_path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    if reject_long_path_component(old_path.as_str()).is_err()
        || reject_long_path_component(new_path.as_str()).is_err()
    {
        return UserRet::from_error(ErrNo::ENAMETOOLONG);
    }
    let old_resolved = match resolve_path_at(old_dirfd, old_path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let new_resolved = match resolve_path_at(new_dirfd, new_path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    if old_resolved == new_resolved {
        return UserRet::from_success(0);
    }
    if path_is_beneath(new_resolved.as_str(), old_resolved.as_str()) {
        match active_impl::backend().metadata(old_resolved.as_str()) {
            Ok(meta) if meta.node_type == VfsNodeType::Directory => {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            Ok(_) => {}
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    }
    if let Err(e) = check_existing_type_mismatch(old_resolved.as_str(), new_resolved.as_str()) {
        return UserRet::from_error(e);
    }

    if flags & RENAME_EXCHANGE != 0 {
        if let Err(e) = check_rename_permission(old_resolved.as_str(), new_resolved.as_str(), true) {
            return UserRet::from_error(e);
        }
        return match rename_exchange(old_resolved.as_str(), new_resolved.as_str()) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        };
    }

    if flags & RENAME_NOREPLACE != 0 {
        match active_impl::backend().metadata(new_resolved.as_str()) {
            Ok(_) => return UserRet::from_error(ErrNo::EEXIST),
            Err(VfsError::NotFound) => {}
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    }
    if let Err(e) = check_rename_permission(old_resolved.as_str(), new_resolved.as_str(), false) {
        return UserRet::from_error(e);
    }

    match vfs::rename_absolute(old_resolved.as_str(), new_resolved.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn reject_long_path_component(path: &str) -> Result<(), ErrNo> {
    if path.split('/').any(|component| component.len() > NAME_MAX) {
        Err(ErrNo::ENAMETOOLONG)
    } else {
        Ok(())
    }
}

fn check_existing_type_mismatch(old_path: &str, new_path: &str) -> Result<(), ErrNo> {
    let old_meta = active_impl::backend()
        .metadata(old_path)
        .map_err(vfs_error_to_errno)?;
    match active_impl::backend().metadata(new_path) {
        Ok(new_meta)
            if old_meta.node_type == VfsNodeType::Directory
                && new_meta.node_type != VfsNodeType::Directory =>
        {
            Err(ErrNo::ENOTDIR)
        }
        Ok(_) | Err(VfsError::NotFound) => Ok(()),
        Err(e) => Err(vfs_error_to_errno(e)),
    }
}

fn path_is_beneath(path: &str, parent: &str) -> bool {
    if parent == "/" {
        return path != "/";
    }
    path.len() > parent.len()
        && path.as_bytes().get(parent.len()) == Some(&b'/')
        && path.starts_with(parent)
}

fn check_rename_permission(old_path: &str, new_path: &str, exchange: bool) -> Result<(), ErrNo> {
    let cred = cred::current_credentials();
    if cred.effective_uid.0 == 0 {
        return Ok(());
    }
    let old_parent = parent_path(old_path);
    let new_parent = parent_path(new_path);
    let old_parent_meta = check_parent_write_search(old_parent, &cred)?;
    let new_parent_meta = if new_parent == old_parent {
        old_parent_meta.clone()
    } else {
        check_parent_write_search(new_parent, &cred)?
    };
    let old_meta = active_impl::backend()
        .metadata(old_path)
        .map_err(vfs_error_to_errno)?;
    check_sticky_parent(&old_parent_meta, &old_meta, &cred)?;
    match active_impl::backend().metadata(new_path) {
        Ok(new_meta) => {
            if exchange || new_parent_meta.mode & 0o1000 != 0 {
                check_sticky_parent(&new_parent_meta, &new_meta, &cred)?;
            }
        }
        Err(VfsError::NotFound) => {}
        Err(e) => return Err(vfs_error_to_errno(e)),
    }
    Ok(())
}

fn check_parent_write_search(path: &str, cred: &ProcessCredentials) -> Result<VfsMetadata, ErrNo> {
    let meta = match active_impl::backend().metadata(path) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => meta,
        Ok(_) => return Err(ErrNo::ENOTDIR),
        Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
        Err(e) => return Err(vfs_error_to_errno(e)),
    };
    if can_write_search_directory(&meta, cred) {
        Ok(meta)
    } else {
        Err(ErrNo::EACCES)
    }
}

fn check_sticky_parent(
    parent_meta: &VfsMetadata,
    child_meta: &VfsMetadata,
    cred: &ProcessCredentials,
) -> Result<(), ErrNo> {
    if parent_meta.mode & 0o1000 == 0
        || cred.effective_uid.0 == parent_meta.uid
        || cred.effective_uid.0 == child_meta.uid
    {
        Ok(())
    } else {
        Err(ErrNo::EPERM)
    }
}

fn can_write_search_directory(meta: &VfsMetadata, cred: &ProcessCredentials) -> bool {
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

fn cred_has_group(cred: &ProcessCredentials, gid: Gid) -> bool {
    cred.effective_gid == gid
        || cred
            .supplementary_groups
            .iter()
            .take(cred.supplementary_group_len)
            .any(|group| *group == gid)
}

fn parent_path(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/")
}

fn rename_exchange(old_path: &str, new_path: &str) -> Result<(), ErrNo> {
    active_impl::backend()
        .metadata(old_path)
        .map_err(vfs_error_to_errno)?;
    active_impl::backend()
        .metadata(new_path)
        .map_err(vfs_error_to_errno)?;

    let temp_path = exchange_temp_path(old_path);
    vfs::rename_absolute(old_path, temp_path.as_str()).map_err(vfs_error_to_errno)?;
    if let Err(e) = vfs::rename_absolute(new_path, old_path) {
        let _ = vfs::rename_absolute(temp_path.as_str(), old_path);
        return Err(vfs_error_to_errno(e));
    }
    if let Err(e) = vfs::rename_absolute(temp_path.as_str(), new_path) {
        let _ = vfs::rename_absolute(old_path, new_path);
        let _ = vfs::rename_absolute(temp_path.as_str(), old_path);
        return Err(vfs_error_to_errno(e));
    }
    Ok(())
}

fn exchange_temp_path(old_path: &str) -> alloc::string::String {
    let parent = old_path
        .rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/");
    let id = task::current_task_id().unwrap_or(0);
    let tick = task::current_tick();
    if parent == "/" {
        format!("/.wateros-rename-exchange-{id}-{tick}")
    } else {
        format!("{parent}/.wateros-rename-exchange-{id}-{tick}")
    }
}
