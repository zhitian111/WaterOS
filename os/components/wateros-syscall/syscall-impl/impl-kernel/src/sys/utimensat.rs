//! `utimensat(2)`：bring-up 最小兼容实现，暂不持久化 atime/mtime。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use platform::wall_clock::realtime_ns;
use vfs::active_impl;
use vfs::api::SingleRootReadView;

use crate::sys::stat_times::{self, StatTime};
use crate::sys::path_at::resolve_path_at;
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
        if dirfd < 0 {
            return UserRet::from_error(ErrNo::EBADF);
        }
        return match vfs::fd::with_current_io(dirfd as usize, |handle| handle.metadata()) {
            Ok(meta) => match apply_times(&meta, updates) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            },
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    match active_impl::backend().metadata(resolved.as_str()) {
        Ok(meta) => match apply_times(&meta, updates) {
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

fn apply_times(meta: &vfs::api::VfsMetadata, updates: [TimeUpdate; 2]) -> Result<(), ErrNo> {
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
