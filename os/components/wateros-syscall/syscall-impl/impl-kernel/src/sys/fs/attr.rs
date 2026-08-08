use crate::alloc::string::ToString;
use crate::sys::path_at::{resolve_path_at, resolve_symlinks, AT_FDCWD};
use crate::sys::stat_times::{self, StatTime};
use crate::user_copy::{copy_from_user_struct, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::{Gid, ProcessCredentials, Uid};
use vfs::active_impl;
use vfs::api::{FinalSymlink, VfsError, VfsMetadata, VfsNodeType};
use vfs::SingleRootReadView;

const FCHOWNAT_VALID_FLAGS : u32 = 0x1000 | 0x100; // AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW
const AT_EMPTY_PATH : u32 = 0x1000;
const AT_SYMLINK_NOFOLLOW : u32 = 0x100;
const UTIMENSAT_VALID_FLAGS : u32 = AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW;
const UTIME_NOW : isize = (1 << 30) - 1;
const UTIME_OMIT : isize = (1 << 30) - 2;
const CHOWN_OMIT_ID : u32 = !0u32 as u32;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

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
    let resolved = match resolve_symlinks(resolved.as_str(), FinalSymlink::Follow) {
        Ok(path) => path,
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
    let final_mode = if flags & AT_SYMLINK_NOFOLLOW != 0 {
        FinalSymlink::NoFollow
    } else {
        FinalSymlink::Follow
    };

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
    let resolved = match resolve_symlinks(resolved.as_str(), final_mode) {
        Ok(path) => path,
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

pub(crate) fn sys_utimensat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let times_ptr = args.arg(2);
    let flags = args.arg(3) as u32;
    if flags & !UTIMENSAT_VALID_FLAGS != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let now = match current_realtime() {
        Ok(now) => now,
        Err(error) => return UserRet::from_error(error),
    };
    let (atime, mtime, times_are_now) = match read_requested_times(times_ptr, now) {
        Ok(times) => times,
        Err(error) => return UserRet::from_error(error),
    };
    let final_symlink = if flags & AT_SYMLINK_NOFOLLOW != 0 {
        FinalSymlink::NoFollow
    } else {
        FinalSymlink::Follow
    };
    let (_path, meta) = match resolve_utimens_target(dirfd, path_ptr, flags, final_symlink) {
        Ok(target) => target,
        Err(error) => return UserRet::from_error(error),
    };

    if let Err(error) = check_utimens_permission(&meta, times_are_now) {
        return UserRet::from_error(error);
    }
    if atime.is_none() && mtime.is_none() {
        return UserRet::from_success(0);
    }
    if let Err(error) = vfs::assert_path_writable(_path.as_str()) {
        return UserRet::from_error(vfs_error_to_errno(error));
    }

    // VFS metadata does not yet expose writable timestamps. Keep the values in
    // the existing inode-keyed syscall sidecar so stat/statx observe Linux
    // utimensat semantics without coupling this syscall to an ext4 backend.
    stat_times::set(&meta, atime, mtime);
    UserRet::from_success(0)
}

fn current_realtime() -> Result<StatTime, ErrNo> {
    let ns = platform::wall_clock::realtime_ns().map_err(|_| ErrNo::EIO)?;
    Ok(StatTime { sec : (ns / 1_000_000_000) as i64,
                  nsec : (ns % 1_000_000_000) as i64 })
}

fn read_requested_times(times_ptr : usize,
                        now : StatTime)
                        -> Result<(Option<StatTime>, Option<StatTime>, bool), ErrNo> {
    if times_ptr == 0 {
        return Ok((Some(now), Some(now), true));
    }
    let atime = copy_from_user_struct::<UserTimespec>(times_ptr)?;
    let mtime =
        copy_from_user_struct::<UserTimespec>(times_ptr + core::mem::size_of::<UserTimespec>())?;
    let atime = parse_requested_time(atime, now)?;
    let mtime = parse_requested_time(mtime, now)?;
    let times_are_now = matches!(atime,
                                 RequestedTime::Now(_) | RequestedTime::Omit) &&
                        matches!(mtime,
                                 RequestedTime::Now(_) | RequestedTime::Omit);
    Ok((atime.value(), mtime.value(), times_are_now))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestedTime {
    Value(StatTime),
    Now(StatTime),
    Omit,
}

impl RequestedTime {
    fn value(self) -> Option<StatTime> {
        match self {
            Self::Value(value) => Some(value),
            Self::Now(value) => Some(value),
            Self::Omit => None,
        }
    }
}

fn parse_requested_time(value : UserTimespec, now : StatTime) -> Result<RequestedTime, ErrNo> {
    match value.nsec {
        UTIME_NOW => Ok(RequestedTime::Now(now)),
        UTIME_OMIT => Ok(RequestedTime::Omit),
        0..=999_999_999 => Ok(RequestedTime::Value(StatTime { sec : value.sec as i64,
                                                              nsec : value.nsec as i64 })),
        _ => Err(ErrNo::EINVAL),
    }
}

fn resolve_utimens_target(dirfd : isize,
                          path_ptr : usize,
                          flags : u32,
                          final_symlink : FinalSymlink)
                          -> Result<(alloc::string::String, VfsMetadata), ErrNo> {
    if path_ptr == 0 {
        if dirfd == AT_FDCWD {
            return Err(ErrNo::EFAULT);
        }
        if flags != 0 {
            return Err(ErrNo::EINVAL);
        }
        if dirfd < 0 {
            return Err(ErrNo::EBADF);
        }
        return vfs::fd::with_current_io(dirfd as usize, |handle| {
                   let path = handle.backing_path()
                                    .map(ToString::to_string)
                                    .ok_or(VfsError::Unsupported)?;
                   Ok((path, handle.metadata()?))
               }).map_err(vfs_error_to_errno);
    }
    let path = copy_user_path_cstr(path_ptr,
                                   crate::user_copy::USER_PATH_MAX)?;
    if path.is_empty() {
        if flags & AT_EMPTY_PATH == 0 {
            return Err(ErrNo::ENOENT);
        }
        if dirfd < 0 {
            return Err(ErrNo::EBADF);
        }
        return vfs::fd::with_current_io(dirfd as usize, |handle| {
                   let path = handle.backing_path()
                                    .map(ToString::to_string)
                                    .ok_or(VfsError::Unsupported)?;
                   Ok((path, handle.metadata()?))
               }).map_err(vfs_error_to_errno);
    }

    let resolved = resolve_path_at(dirfd, path.as_str())?;
    let resolved = resolve_symlinks(resolved.as_str(), final_symlink)?;
    let meta = active_impl::backend().metadata(resolved.as_str())
                                     .map_err(vfs_error_to_errno)?;
    Ok((resolved, meta))
}

fn check_utimens_permission(meta : &VfsMetadata, times_are_now : bool) -> Result<(), ErrNo> {
    let credentials = cred::current_credentials();
    if credentials.effective_uid
                  .0 ==
       0 ||
       credentials.effective_uid
                  .0 ==
       meta.uid
    {
        return Ok(());
    }
    if !times_are_now {
        return Err(ErrNo::EPERM);
    }
    let write_bit = if credentials.effective_gid
                                  .0 ==
                       meta.gid ||
                       cred_has_group(&credentials, Gid(meta.gid))
    {
        0o020
    } else {
        0o002
    };
    if meta.mode as u32 & write_bit != 0 {
        Ok(())
    } else {
        Err(ErrNo::EACCES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW : StatTime = StatTime { sec : 123,
                                      nsec : 456 };

    #[test]
    fn utimens_time_markers_are_parsed_without_using_tv_sec() {
        assert_eq!(parse_requested_time(UserTimespec { sec : -1,
                                                       nsec : UTIME_NOW },
                                        NOW),
                   Ok(RequestedTime::Now(NOW)));
        assert_eq!(parse_requested_time(UserTimespec { sec : -1,
                                                       nsec : UTIME_OMIT },
                                        NOW),
                   Ok(RequestedTime::Omit));
    }

    #[test]
    fn utimens_explicit_time_accepts_negative_seconds_and_validates_nanoseconds() {
        assert_eq!(parse_requested_time(UserTimespec { sec : -10,
                                                       nsec : 999_999_999 },
                                        NOW),
                   Ok(RequestedTime::Value(StatTime { sec : -10,
                                                      nsec : 999_999_999 })));
        assert_eq!(parse_requested_time(UserTimespec { sec : 0,
                                                       nsec : -1 },
                                        NOW),
                   Err(ErrNo::EINVAL));
        assert_eq!(parse_requested_time(UserTimespec { sec : 0,
                                                       nsec : 1_000_000_000 },
                                        NOW),
                   Err(ErrNo::EINVAL));
    }
}
