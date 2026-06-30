//! `utimensat(2)`：bring-up 最小兼容实现，暂不持久化 atime/mtime。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::string::ToString;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, ProcessCredentials};
use platform::wall_clock::realtime_ns;
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsMetadata};

use crate::sys::stat_times::{self, StatTime};
use crate::sys::path_at::{resolve_final_symlink, resolve_path_at};
use crate::user_copy::{copy_from_user_struct, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;

const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
const UTIME_NOW: isize = 1_073_741_823;
const UTIME_OMIT: isize = 1_073_741_822;

#[repr(C)]
#[derive(Clone, Copy)]
// 本结构代码由AI完成
struct UserTimespec {
    sec: isize,
    nsec: isize,
}

#[derive(Clone, Copy)]
enum TimeUpdate {
    Set(StatTime),
    Now,
    Omit,
}

// 本方法代码由AI完成
pub(crate) fn sys_utimensat(args: SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let times_ptr = args.arg(2);
    let flags = args.arg(3) as u32;

    if flags & !AT_SYMLINK_NOFOLLOW != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let updates = match read_timespec_pair(times_ptr) {
        Ok(updates) => updates,
        Err(e) => return UserRet::from_error(e),
    };
    if both_omitted(&updates) {
        return UserRet::from_success(0);
    }

    if path_ptr == 0 {
        if flags != 0 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        if dirfd < 0 {
            return UserRet::from_error(ErrNo::EBADF);
        }
        return match vfs::fd::with_current_io(dirfd as usize, |handle| {
            let path = handle.backing_path().map(|path| path.to_string());
            let meta = handle.metadata()?;
            Ok((path, meta))
        }) {
            Ok((path, meta)) => match apply_times(path.as_deref(), &meta, updates) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            },
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = if flags & AT_SYMLINK_NOFOLLOW != 0 {
        resolved
    } else {
        match resolve_final_symlink(resolved.as_str()) {
            Ok(path) => path,
            Err(e) => return UserRet::from_error(e),
        }
    };

    match active_impl::backend().metadata(resolved.as_str()) {
        Ok(meta) => match apply_times(Some(resolved.as_str()), &meta, updates) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        },
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn read_timespec_pair(times_ptr: usize) -> Result<[TimeUpdate; 2], ErrNo> {
    if times_ptr == 0 {
        return Ok([TimeUpdate::Now, TimeUpdate::Now]);
    }
    let step = core::mem::size_of::<UserTimespec>();
    let mut out = [TimeUpdate::Omit; 2];
    for i in 0..2 {
        let ts = copy_from_user_struct::<UserTimespec>(times_ptr + i * step)?;
        out[i] = timespec_update(ts)?;
    }
    Ok(out)
}

fn timespec_update(ts: UserTimespec) -> Result<TimeUpdate, ErrNo> {
    if ts.nsec == UTIME_NOW {
        return Ok(TimeUpdate::Now);
    }
    if ts.nsec == UTIME_OMIT {
        return Ok(TimeUpdate::Omit);
    }
    if ts.sec < 0 || ts.nsec < 0 || ts.nsec >= 1_000_000_000 {
        return Err(ErrNo::EINVAL);
    }
    Ok(TimeUpdate::Set(StatTime {
        sec: ts.sec as i64,
        nsec: ts.nsec as i64,
    }))
}

fn both_omitted(updates: &[TimeUpdate; 2]) -> bool {
    matches!(updates[0], TimeUpdate::Omit) && matches!(updates[1], TimeUpdate::Omit)
}

fn apply_times(path: Option<&str>, meta: &VfsMetadata, updates: [TimeUpdate; 2]) -> Result<(), ErrNo> {
    if let Some(path) = path {
        check_writable_mount(path, meta)?;
    }
    check_time_update_permission(meta, &updates)?;
    let now = if matches!(updates[0], TimeUpdate::Now) || matches!(updates[1], TimeUpdate::Now) {
        Some(now_time()?)
    } else {
        None
    };
    let atime = resolve_update(updates[0], now);
    let mtime = resolve_update(updates[1], now);
    stat_times::set(meta, atime, mtime);
    Ok(())
}

fn check_writable_mount(path: &str, meta: &VfsMetadata) -> Result<(), ErrNo> {
    match vfs::chmod_absolute(path, (meta.mode as u32) & 0o7777) {
        Ok(()) => Ok(()),
        Err(vfs::api::VfsError::ReadOnlyFs) => Err(ErrNo::EROFS),
        Err(_) => Ok(()),
    }
}

fn check_time_update_permission(meta: &VfsMetadata, updates: &[TimeUpdate; 2]) -> Result<(), ErrNo> {
    let cred = cred::current_credentials();
    if cred.effective_uid.0 == 0 || cred.effective_uid.0 == meta.uid {
        return Ok(());
    }
    if updates_are_now(updates) && can_write_file(&cred, meta) {
        return Ok(());
    }
    if updates_are_now(updates) {
        Err(ErrNo::EACCES)
    } else {
        Err(ErrNo::EPERM)
    }
}

fn updates_are_now(updates: &[TimeUpdate; 2]) -> bool {
    matches!(updates[0], TimeUpdate::Now) && matches!(updates[1], TimeUpdate::Now)
}

fn can_write_file(cred: &ProcessCredentials, meta: &VfsMetadata) -> bool {
    if cred.effective_uid.0 == 0 {
        return true;
    }
    if cred.effective_uid.0 == meta.uid {
        return meta.mode & 0o200 != 0;
    }
    if cred_has_group(cred, Gid(meta.gid)) {
        return meta.mode & 0o020 != 0;
    }
    meta.mode & 0o002 != 0
}

fn cred_has_group(cred: &ProcessCredentials, gid: Gid) -> bool {
    cred.effective_gid == gid
        || cred
            .supplementary_groups
            .iter()
            .take(cred.supplementary_group_len)
            .any(|group| *group == gid)
}

fn resolve_update(update: TimeUpdate, now: Option<StatTime>) -> Option<StatTime> {
    match update {
        TimeUpdate::Set(time) => Some(time),
        TimeUpdate::Now => now,
        TimeUpdate::Omit => None,
    }
}

fn now_time() -> Result<StatTime, ErrNo> {
    let ns = realtime_ns().map_err(|_| ErrNo::EIO)?;
    Ok(StatTime {
        sec: (ns / 1_000_000_000) as i64,
        nsec: (ns % 1_000_000_000) as i64,
    })
}
