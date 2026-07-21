//! `fchmodat(2)`：相对目录修改路径权限（首期支持 `resolve_path_at` 路径解析）。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::string::ToString;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata, VfsNodeType};

use super::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

// 本方法代码由AI完成
pub(crate) fn sys_fchmodat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let mut mode = (args.arg(2) as u32) & 0o7777;
    let flags = args.arg(3) as u32;

    if flags != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let path = match copy_user_path_cstr(path_ptr,
                                         crate::user_copy::USER_PATH_MAX)
    {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let meta = match active_impl::backend().metadata(resolved.as_str()) {
        Ok(meta) => meta,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };
    if let Err(errno) = ensure_chmod_owner(&meta) {
        return UserRet::from_error(errno);
    }
    mode = adjust_chmod_mode(mode, &meta);

    match vfs::chmod_absolute(resolved.as_str(), mode) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Unsupported) => UserRet::from_error(ErrNo::EPERM),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_fchmod(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let mut mode = (args.arg(1) as u32) & 0o7777;

    match vfs::fd::is_path_only_fd(fd) {
        Ok(true) => return UserRet::from_error(ErrNo::EBADF),
        Ok(false) => {}
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    }

    let (path, meta) = match vfs::fd::with_current_io(fd, |handle| {
              let path = handle.backing_path()
                               .ok_or(vfs::api::VfsError::Unsupported)?
                               .to_string();
              let meta = handle.metadata()?;
              Ok((path, meta))
          }) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };

    if let Err(e) = vfs::chmod_absolute(path.as_str(),
                                        (meta.mode as u32) & 0o7777)
    {
        return UserRet::from_error(match e {
            VfsError::Unsupported => ErrNo::EPERM,
            other => vfs_error_to_errno(other),
        });
    }
    if let Err(errno) = ensure_chmod_owner(&meta) {
        return UserRet::from_error(errno);
    }
    mode = adjust_chmod_mode(mode, &meta);

    match vfs::chmod_absolute(path.as_str(), mode) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Unsupported) => UserRet::from_error(ErrNo::EPERM),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn ensure_chmod_owner(meta : &VfsMetadata) -> Result<(), ErrNo> {
    let cred = cred::current_credentials();
    if cred.effective_uid.0 == 0 || cred.effective_uid.0 == meta.uid {
        Ok(())
    } else {
        Err(ErrNo::EPERM)
    }
}

fn adjust_chmod_mode(mut mode : u32, meta : &VfsMetadata) -> u32 {
    if mode & 0o2000 != 0 {
        let cred = cred::current_credentials();
        if meta.node_type == VfsNodeType::Directory &&
           cred.effective_uid.0 != 0 &&
           !cred_has_group(&cred, Gid(meta.gid))
        {
            mode &= !0o2000;
        }
    }
    mode
}

fn cred_has_group(cred : &ProcessCredentials, gid : Gid) -> bool {
    cred.effective_gid == gid ||
    cred.supplementary_groups
        .iter()
        .take(cred.supplementary_group_len)
        .any(|group| *group == gid)
}
