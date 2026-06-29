//! `fchownat(2)`：相对目录修改路径 uid/gid（首期支持 `resolve_path_at` 路径解析）。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, Uid};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError};

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
const AT_EMPTY_PATH: u32 = 0x1000;
const FCHOWNAT_VALID_FLAGS: u32 = AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH;

/// Linux `fchownat` / `lchown` 用 `(uid_t)-1` / `(gid_t)-1` 表示不修改对应字段。
const CHOWN_OMIT_ID: u32 = u32::MAX;

// 本方法代码由AI完成
pub(crate) fn sys_fchownat(args: SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let uid = parse_chown_id(args.arg(2));
    let gid = parse_chown_id(args.arg(3));
    let flags = args.arg(4) as u32;

    if flags & !FCHOWNAT_VALID_FLAGS != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & AT_EMPTY_PATH != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let _nofollow = flags & AT_SYMLINK_NOFOLLOW != 0;

    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    if uid.is_some() || gid.is_some() {
        let meta = match active_impl::backend().metadata(resolved.as_str()) {
            Ok(meta) => meta,
            Err(VfsError::NotAFile) => return UserRet::from_error(ErrNo::ENOTDIR),
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        };
        let cred = cred::current_credentials();
        if !cred::may_chown(
            &cred,
            Uid(meta.uid),
            Gid(meta.gid),
            uid,
            gid,
        ) {
            return UserRet::from_error(ErrNo::EPERM);
        }
    }

    match vfs::chown_absolute(resolved.as_str(), uid, gid) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Unsupported) => UserRet::from_error(ErrNo::EPERM),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn parse_chown_id(arg: usize) -> Option<u32> {
    let id = arg as u32;
    if id == CHOWN_OMIT_ID {
        None
    } else {
        Some(id)
    }
}
