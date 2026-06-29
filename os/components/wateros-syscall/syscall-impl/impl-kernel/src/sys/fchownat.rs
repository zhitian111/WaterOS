//! `fchownat(2)`：相对目录修改路径 uid/gid（首期支持 `resolve_path_at` 路径解析）。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::string::ToString;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, Uid};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata, VfsNodeType};

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

    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    chown_path(resolved.as_str(), uid, gid)
}

// 本方法代码由AI完成
pub(crate) fn sys_fchown(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let uid = parse_chown_id(args.arg(1));
    let gid = parse_chown_id(args.arg(2));

    match vfs::fd::is_path_only_fd(fd) {
        Ok(true) => return UserRet::from_error(ErrNo::EBADF),
        Ok(false) => {}
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    }

    let path = match vfs::fd::with_current_io(fd, |handle| {
        handle
            .backing_path()
            .map(|path| path.to_string())
            .ok_or(VfsError::Unsupported)
    }) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };

    chown_path(path.as_str(), uid, gid)
}

fn chown_path(path: &str, uid: Option<u32>, gid: Option<u32>) -> UserRet {
    if uid.is_some() || gid.is_some() {
        let meta = match active_impl::backend().metadata(path) {
            Ok(meta) => meta,
            Err(VfsError::NotAFile) => return UserRet::from_error(ErrNo::ENOTDIR),
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        };
        if let Err(e) = check_writable_mount(path, &meta) {
            return UserRet::from_error(e);
        }
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

        return match vfs::chown_absolute(path, uid, gid) {
            Ok(()) => match apply_chown_mode_fixup(path, &meta) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            },
            Err(VfsError::Unsupported) => UserRet::from_error(ErrNo::EPERM),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    match vfs::chown_absolute(path, uid, gid) {
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

fn check_writable_mount(path: &str, meta: &VfsMetadata) -> Result<(), ErrNo> {
    match vfs::chmod_absolute(path, (meta.mode as u32) & 0o7777) {
        Ok(()) => Ok(()),
        Err(VfsError::Unsupported) => Err(ErrNo::EPERM),
        Err(e) => Err(vfs_error_to_errno(e)),
    }
}

fn apply_chown_mode_fixup(path: &str, meta: &VfsMetadata) -> Result<(), ErrNo> {
    if meta.node_type != VfsNodeType::File {
        return Ok(());
    }
    let original = (meta.mode as u32) & 0o7777;
    let mut mode = original & !0o4000;
    if mode & 0o0010 != 0 {
        mode &= !0o2000;
    }
    if mode == original {
        return Ok(());
    }
    match vfs::chmod_absolute(path, mode) {
        Ok(()) => Ok(()),
        Err(VfsError::Unsupported) => Err(ErrNo::EPERM),
        Err(e) => Err(vfs_error_to_errno(e)),
    }
}
