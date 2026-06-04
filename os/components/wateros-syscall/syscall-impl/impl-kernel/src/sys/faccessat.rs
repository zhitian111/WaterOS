//! `faccessat(2)`：检查相对目录路径是否存在/可访问。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::SingleRootReadView;

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const F_OK: u32 = 0;
const X_OK: u32 = 1;
const W_OK: u32 = 2;
const R_OK: u32 = 4;

const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
const AT_EACCESS: u32 = 0x200;

pub(crate) fn sys_faccessat(args: SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let mode = args.arg(2) as u32;
    let flags = args.arg(3) as u32;

    if mode & !(R_OK | W_OK | X_OK) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & !(AT_SYMLINK_NOFOLLOW | AT_EACCESS) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let path = match copy_user_path_cstr(path_ptr, 256) {
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

    if mode == F_OK || access_mode_allowed(meta.mode, mode) {
        UserRet::from_success(0)
    } else {
        UserRet::from_error(ErrNo::EACCES)
    }
}

fn access_mode_allowed(file_mode: u16, access_mode: u32) -> bool {
    let mode = u32::from(file_mode);
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
