//! `faccessat(2)` / `faccessat2(2)`：检查相对目录路径是否存在/可访问。
//!
//! Linux `faccessat(48)` 为三参数 syscall，内核固定 `flags=0`、忽略用户态 a3。
//! `faccessat2(439)` 才携带 flags；`AT_SYMLINK_NOFOLLOW` 已生效（不 follow 末端 symlink）。

//! 本模块代码由AI完成
extern crate alloc;

use alloc::string::String;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::ProcessCredentials;
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsNodeType};

use super::path_at::{resolve_path_at, resolve_symlinks};
use vfs::api::FinalSymlink;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const F_OK : u32 = 0;
const X_OK : u32 = 1;
const W_OK : u32 = 2;
const R_OK : u32 = 4;

const AT_SYMLINK_NOFOLLOW : u32 = 0x100;
const AT_EACCESS : u32 = 0x200;
const AT_EMPTY_PATH : u32 = 0x1000;

const FACCESSAT2_VALID_FLAGS : u32 = AT_SYMLINK_NOFOLLOW | AT_EACCESS | AT_EMPTY_PATH;

/// Linux `faccessat(48)`：不读取第 4 个参数（flags 恒为 0）。
// 本方法代码由AI完成
pub(crate) fn sys_faccessat(args : SyscallArgs) -> UserRet {
    do_faccessat(args.arg(0) as isize,
                 args.arg(1),
                 args.arg(2) as u32,
                 0)
}

/// Linux `faccessat2(439)`：完整 flags 参数。
// 本方法代码由AI完成
pub(crate) fn sys_faccessat2(args : SyscallArgs) -> UserRet {
    let flags = args.arg(3) as u32;
    if flags & !FACCESSAT2_VALID_FLAGS != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    do_faccessat(args.arg(0) as isize,
                 args.arg(1),
                 args.arg(2) as u32,
                 flags)
}

fn do_faccessat(dirfd : isize, path_ptr : usize, mode : u32, flags : u32) -> UserRet {
    if mode & !(R_OK | W_OK | X_OK) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let use_effective = flags & AT_EACCESS != 0;
    let nofollow = flags & AT_SYMLINK_NOFOLLOW != 0;

    let file_mode = if path_ptr == 0 && (flags & AT_EMPTY_PATH) != 0 && dirfd >= 0 {
        match vfs::fd::with_current_io(dirfd as usize, |handle| {
                  handle.metadata()
              }) {
            Ok(meta) => meta.mode,
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    } else {
        let path = match copy_user_path_cstr(path_ptr,
                                             crate::user_copy::USER_PATH_MAX)
        {
            Ok(p) => p,
            Err(e) => return UserRet::from_error(e),
        };
        if path.is_empty() && (flags & AT_EMPTY_PATH) != 0 && dirfd >= 0 {
            match vfs::fd::with_current_io(dirfd as usize, |handle| {
                      handle.metadata()
                  }) {
                Ok(meta) => meta.mode,
                Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
            }
        } else {
            if path.is_empty() {
                return UserRet::from_error(ErrNo::ENOENT);
            }
            let resolved = match resolve_path_at(dirfd, path.as_str()) {
                Ok(p) => p,
                Err(e) => return UserRet::from_error(e),
            };
            let cred = cred::current_credentials();
            if let Err(e) = check_parent_search(resolved.as_str(), &cred, use_effective) {
                return UserRet::from_error(e);
            }
            let resolved = if nofollow {
                match resolve_symlinks(resolved.as_str(), FinalSymlink::NoFollow) {
                    Ok(path) => path,
                    Err(e) => return UserRet::from_error(e),
                }
            } else {
                match resolve_symlinks(resolved.as_str(), FinalSymlink::Follow) {
                    Ok(followed) => followed,
                    Err(e) => return UserRet::from_error(e),
                }
            };
            if mode & W_OK != 0 {
                if let Err(e) = vfs::assert_path_writable(resolved.as_str()) {
                    return UserRet::from_error(vfs_error_to_errno(e));
                }
            }
            match active_impl::backend().metadata(resolved.as_str()) {
                Ok(meta) => meta.mode,
                Err(VfsError::NotAFile) => return UserRet::from_error(ErrNo::ENOTDIR),
                Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
            }
        }
    };

    let cred = cred::current_credentials();
    // TODO(cred-vfs): VfsMetadata 尚无 inode owner；后续应改为
    // cred::may_access_inode(cred, owner_uid, owner_gid, mode, access_mask)。
    if mode == F_OK || access_mode_allowed(file_mode, mode, &cred, use_effective) {
        UserRet::from_success(0)
    } else {
        UserRet::from_error(ErrNo::EACCES)
    }
}

fn access_mode_allowed(file_mode : u16,
                       access_mode : u32,
                       cred : &ProcessCredentials,
                       use_effective : bool)
                       -> bool {
    let uid = if use_effective {
        cred.effective_uid.0
    } else {
        cred.real_uid.0
    };

    let mode = u32::from(file_mode);

    // Linux：超级用户 R/W 恒通过；X 在任一 execute 位存在时通过。
    if uid == 0 {
        if access_mode & X_OK != 0 {
            return mode & 0o111 != 0;
        }
        return true;
    }

    // VFS 元数据尚无 inode uid/gid：退化为「任一类别的 permission 位满足即可」。
    if access_mode & R_OK != 0 && mode & 0o444 == 0 {
        return false;
    }
    if access_mode & W_OK != 0 && mode & 0o222 == 0 {
        return false;
    }
    if access_mode & X_OK != 0 && mode & 0o111 == 0 {
        return false;
    }
    true
}

fn check_parent_search(path : &str,
                       cred : &ProcessCredentials,
                       use_effective : bool)
                       -> Result<(), ErrNo> {
    let uid = if use_effective {
        cred.effective_uid.0
    } else {
        cred.real_uid.0
    };
    if uid == 0 {
        return Ok(());
    }

    let parts : alloc::vec::Vec<&str> = path.trim_start_matches('/')
                                            .split('/')
                                            .filter(|part| !part.is_empty())
                                            .collect();
    if parts.len() <= 1 {
        return Ok(());
    }

    let mut current = String::from("/");
    for part in &parts[..parts.len() - 1] {
        if current != "/" {
            current.push('/');
        }
        current.push_str(part);
        match active_impl::backend().metadata(current.as_str()) {
            Ok(meta) if meta.node_type == VfsNodeType::Directory => {
                if !access_mode_allowed(meta.mode, X_OK, cred, use_effective) {
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
