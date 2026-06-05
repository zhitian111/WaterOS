//! `utimensat(2)`：bring-up 最小兼容实现，暂不持久化 atime/mtime。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::SingleRootReadView;

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::{copy_from_user_struct, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;

const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
const UTIME_NOW: isize = 1_073_741_823;
const UTIME_OMIT: isize = 1_073_741_822;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimespec {
    sec: isize,
    nsec: isize,
}

pub(crate) fn sys_utimensat(args: SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let times_ptr = args.arg(2);
    let flags = args.arg(3) as u32;

    if flags & !AT_SYMLINK_NOFOLLOW != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if let Err(e) = validate_timespec_pair(times_ptr) {
        return UserRet::from_error(e);
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
        Ok(_) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn validate_timespec_pair(times_ptr: usize) -> Result<(), ErrNo> {
    if times_ptr == 0 {
        return Ok(());
    }
    let step = core::mem::size_of::<UserTimespec>();
    for i in 0..2 {
        let ts = copy_from_user_struct::<UserTimespec>(times_ptr + i * step)?;
        if ts.nsec == UTIME_NOW || ts.nsec == UTIME_OMIT {
            continue;
        }
        if ts.sec < 0 || ts.nsec < 0 || ts.nsec >= 1_000_000_000 {
            return Err(ErrNo::EINVAL);
        }
    }
    Ok(())
}
