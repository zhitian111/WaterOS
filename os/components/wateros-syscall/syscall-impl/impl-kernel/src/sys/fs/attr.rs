use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, ProcessCredentials, Uid};
use vfs::api::{VfsError, VfsMetadata, VfsNodeType};
use vfs::active_impl;
use vfs::SingleRootReadView;
use crate::alloc::string::ToString;
use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const FCHOWNAT_VALID_FLAGS: u32 = 0x1000 | 0x100; // AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW
const AT_EMPTY_PATH: u32 = 0x1000;
const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
const CHOWN_OMIT_ID: u32 = !0u32 as u32;

pub(crate) fn sys_fchmodat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let mut mode = (args.arg(2) as u32) & 0o7777;
    // Linux `fchmodat(2)` (syscall 53) has exactly three arguments.  Do not
    // inspect a3 here: callers are not required to initialize it, and treating
    // its residual value as flags turns ordinary `chmod()` into EINVAL.
    // Flag-bearing semantics belong to the distinct fchmodat2 syscall.

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
pub(crate) fn sys_fchownat(args : SyscallArgs) -> UserRet {
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

    chown_path(resolved.as_str(), uid, gid)
}

// 本方法代码由AI完成
pub(crate) fn sys_fchown(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let uid = parse_chown_id(args.arg(1));
    let gid = parse_chown_id(args.arg(2));

    match vfs::fd::is_path_only_fd(fd) {
        Ok(true) => return UserRet::from_error(ErrNo::EBADF),
        Ok(false) => {}
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    }

    let path = match vfs::fd::with_current_io(fd, |handle| {
              handle.backing_path()
                    .map(|path| path.to_string())
                    .ok_or(VfsError::Unsupported)
          }) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };

    chown_path(path.as_str(), uid, gid)
}

fn chown_path(path : &str, uid : Option<u32>, gid : Option<u32>) -> UserRet {
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
        if !cred::may_chown(&cred,
                            Uid(meta.uid),
                            Gid(meta.gid),
                            uid,
                            gid)
        {
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

fn parse_chown_id(arg : usize) -> Option<u32> {
    let id = arg as u32;
    if id == CHOWN_OMIT_ID {
        None
    } else {
        Some(id)
    }
}

fn check_writable_mount(path : &str, meta : &VfsMetadata) -> Result<(), ErrNo> {
    match vfs::chmod_absolute(path, (meta.mode as u32) & 0o7777) {
        Ok(()) => Ok(()),
        Err(VfsError::Unsupported) => Err(ErrNo::EPERM),
        Err(e) => Err(vfs_error_to_errno(e)),
    }
}

fn apply_chown_mode_fixup(path : &str, meta : &VfsMetadata) -> Result<(), ErrNo> {
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

// 本方法代码由AI完成
pub(crate) fn sys_utimensat(_args: SyscallArgs) -> UserRet {
    UserRet::from_error(ErrNo::ENOSYS)
}
