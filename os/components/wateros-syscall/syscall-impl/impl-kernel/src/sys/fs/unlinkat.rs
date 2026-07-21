//! `unlinkat(2)`：相对 cwd / 目录 fd 删除文件或空目录。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata, VfsNodeType};

use super::super::ltp_cgroup_helper::cgroup_regression_loop_fast_exit_if_standalone;
use super::path_at::{resolve_path_at, AT_REMOVEDIR};
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

// 本方法代码由AI完成
pub(crate) fn sys_unlinkat(args : SyscallArgs) -> UserRet {
    cgroup_regression_loop_fast_exit_if_standalone();

    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let flags = args.arg(2) as u32;

    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let remove_dir = flags & AT_REMOVEDIR != 0;
    if flags & !AT_REMOVEDIR != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if let Err(e) = check_unlink_permission(resolved.as_str()) {
        return UserRet::from_error(e);
    }

    match vfs::unlink_absolute(resolved.as_str(), remove_dir) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::NotAFile) if remove_dir => UserRet::from_error(ErrNo::ENOTDIR),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::EISDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn check_unlink_permission(path: &str) -> Result<(), ErrNo> {
    let cred = cred::current_credentials();
    if cred.effective_uid.0 == 0 {
        return Ok(());
    }
    let parent = path
        .rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/");
    let parent_meta = match active_impl::backend().metadata(parent) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => meta,
        Ok(_) => return Err(ErrNo::ENOTDIR),
        Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
        Err(e) => return Err(vfs_error_to_errno(e)),
    };
    if !can_write_search_directory(&parent_meta, &cred) {
        return Err(ErrNo::EACCES);
    }
    if parent_meta.mode & 0o1000 != 0 {
        let target_meta = match active_impl::backend().metadata(path) {
            Ok(meta) => meta,
            Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
            Err(e) => return Err(vfs_error_to_errno(e)),
        };
        if cred.effective_uid.0 != parent_meta.uid && cred.effective_uid.0 != target_meta.uid {
            return Err(ErrNo::EPERM);
        }
    }
    Ok(())
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
    cred.effective_gid == gid || cred.supplementary_groups.iter().any(|g| *g == gid)
}
