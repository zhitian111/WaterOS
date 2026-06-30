//! `symlinkat(2)`：相对目录 fd 创建符号链接。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsNodeType};

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const PATH_MAX: usize = 4096;

// 本方法代码由AI完成
pub(crate) fn sys_symlinkat(args: SyscallArgs) -> UserRet {
    let target_ptr = args.arg(0);
    let dirfd = args.arg(1) as isize;
    let linkpath_ptr = args.arg(2);

    let target = match copy_user_path_cstr(target_ptr, PATH_MAX) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let linkpath = match copy_user_path_cstr(linkpath_ptr, PATH_MAX) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(dirfd, linkpath.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let cred = cred::current_credentials();
    if let Err(e) = check_parent_create(resolved.as_str(), &cred) {
        return UserRet::from_error(e);
    }

    match vfs::symlink_absolute(target.as_str(), resolved.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Exists) => UserRet::from_error(ErrNo::EEXIST),
        Err(VfsError::NotFound) => UserRet::from_error(ErrNo::ENOENT),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::ENOTDIR),
        Err(VfsError::ReadOnlyFs) => UserRet::from_error(ErrNo::EROFS),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn check_parent_create(path: &str, cred: &ProcessCredentials) -> Result<(), ErrNo> {
    let parent = path
        .rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/");
    let meta = active_impl::backend()
        .metadata(parent)
        .map_err(vfs_error_to_errno)?;
    if meta.node_type != VfsNodeType::Directory {
        return Err(ErrNo::ENOTDIR);
    }
    if cred.effective_uid.0 == 0 {
        return Ok(());
    }
    let mode = u32::from(meta.mode);
    let allowed = if cred.effective_uid.0 == meta.uid {
        mode & 0o300 == 0o300
    } else if cred_has_group(cred, Gid(meta.gid)) {
        mode & 0o030 == 0o030
    } else {
        mode & 0o003 == 0o003
    };
    if allowed {
        Ok(())
    } else {
        Err(ErrNo::EACCES)
    }
}

fn cred_has_group(cred: &ProcessCredentials, gid: Gid) -> bool {
    cred.effective_gid == gid
        || cred
            .supplementary_groups
            .iter()
            .take(cred.supplementary_group_len)
            .any(|group| *group == gid)
}
