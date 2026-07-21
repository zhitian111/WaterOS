//! `chdir(2)`：切换当前任务工作目录。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata, VfsNodeType};

use super::path_at::{resolve_final_symlink, resolve_path_at, AT_FDCWD};
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

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
    let resolved = match resolve_final_symlink(resolved.as_str()) {
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
