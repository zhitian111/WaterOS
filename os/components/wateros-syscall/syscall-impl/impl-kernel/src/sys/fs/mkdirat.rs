//! `mkdirat(2)`：相对 `AT_FDCWD` 或目录 fd 创建目录。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata, VfsNodeType};

use super::super::ltp_cgroup_helper::cgroup_regression_loop_fast_exit_if_standalone;
use super::path_at::resolve_path_at;
use crate::sys::task;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

// 本方法代码由AI完成
pub(crate) fn sys_mkdirat(args: SyscallArgs) -> UserRet {
    cgroup_regression_loop_fast_exit_if_standalone();

    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let mode = args.arg(2) as u32;

    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let cred = cred::current_credentials();
    let parent_meta = match check_parent_create(resolved.as_str(), &cred) {
        Ok(meta) => meta,
        Err(e) => return UserRet::from_error(e),
    };
    let mut create_mode = mode & !task::current_umask() & 0o7777;
    let gid = if parent_meta.mode & 0o2000 != 0 {
        create_mode |= 0o2000;
        parent_meta.gid
    } else {
        cred.effective_gid.0
    };

    match vfs::mkdir_absolute(resolved.as_str(), create_mode) {
        Ok(()) => {
            if let Err(e) =
                vfs::chown_absolute(resolved.as_str(), Some(cred.effective_uid.0), Some(gid))
            {
                return UserRet::from_error(vfs_error_to_errno(e));
            }
            if let Err(e) = vfs::chmod_absolute(resolved.as_str(), create_mode) {
                return UserRet::from_error(vfs_error_to_errno(e));
            }
            UserRet::from_success(0)
        }
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::ENOTDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn check_parent_create(path: &str, cred: &ProcessCredentials) -> Result<VfsMetadata, ErrNo> {
    let parent = path
        .rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/");
    let meta = match active_impl::backend().metadata(parent) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => meta,
        Ok(_) => return Err(ErrNo::ENOTDIR),
        Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
        Err(e) => return Err(vfs_error_to_errno(e)),
    };
    if can_create_in_directory(&meta, cred) {
        Ok(meta)
    } else {
        Err(ErrNo::EACCES)
    }
}

fn can_create_in_directory(meta: &VfsMetadata, cred: &ProcessCredentials) -> bool {
    if cred.effective_uid.0 == 0 {
        return true;
    }
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
