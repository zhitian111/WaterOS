//! `renameat2(2)`：bring-up 同父目录 rename（文件与目录）；非 journal 原子语义。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError};

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const RENAME_NOREPLACE: u32 = 1;

pub(crate) fn sys_renameat2(args: SyscallArgs) -> UserRet {
    let old_dirfd = args.arg(0) as isize;
    let old_path_ptr = args.arg(1);
    let new_dirfd = args.arg(2) as isize;
    let new_path_ptr = args.arg(3);
    let flags = args.arg(4) as u32;

    if flags & !(RENAME_NOREPLACE) != 0 {
        log::warn!(
            "[syscall] renameat2(nr=276) unsupported flags={:#x}",
            flags,
        );
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let old_path = match copy_user_path_cstr(old_path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let new_path = match copy_user_path_cstr(new_path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let old_resolved = match resolve_path_at(old_dirfd, old_path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let new_resolved = match resolve_path_at(new_dirfd, new_path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    if old_resolved == new_resolved {
        return UserRet::from_success(0);
    }

    if flags & RENAME_NOREPLACE != 0 {
        match active_impl::backend().metadata(new_resolved.as_str()) {
            Ok(_) => return UserRet::from_error(ErrNo::EEXIST),
            Err(VfsError::NotFound) => {}
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    }

    match vfs::rename_absolute(old_resolved.as_str(), new_resolved.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
