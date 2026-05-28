//! `fstat(2)`：将已打开文件的元数据写入用户 `stat` 缓冲。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::{
    SingleRootReadView, VFS_FIRST_DYNAMIC_FD, VFS_STDERR_FD, VFS_STDIN_FD,
};

use crate::linux_stat::{fill_linux_stat, fill_linux_statx};
use crate::sys::path_at::resolve_path_at;
use crate::user_copy::{copy_to_user_struct, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;

const AT_EMPTY_PATH : u32 = 0x1000;

pub(crate) fn sys_fstat(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let stat_ptr = args.arg(1);
    if stat_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if fd < VFS_FIRST_DYNAMIC_FD && (fd > VFS_STDERR_FD || fd < VFS_STDIN_FD) {
        return UserRet::from_error(ErrNo::EBADF);
    }

    match vfs::fd::with_current_io(fd, |handle| {
              let meta = handle.metadata()?;
              Ok(fill_linux_stat(&meta, meta.size))
          }) {
        Ok(stat) => match copy_to_user_struct(stat_ptr, &stat) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        },
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

pub(crate) fn sys_statx(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let flags = args.arg(2) as u32;
    let mask = args.arg(3) as u32;
    let statx_ptr = args.arg(4);
    if statx_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let path = if path_ptr == 0 && (flags & AT_EMPTY_PATH) != 0 {
        alloc::string::String::new()
    } else {
        match copy_user_path_cstr(path_ptr, 256) {
            Ok(p) => p,
            Err(e) => return UserRet::from_error(e),
        }
    };

    let statx = if path.is_empty() && (flags & AT_EMPTY_PATH) != 0 && dirfd >= 0 {
        match vfs::fd::with_current_io(dirfd as usize, |handle| {
                  let meta = handle.metadata()?;
                  Ok(fill_linux_statx(&meta, meta.size, mask))
              }) {
            Ok(statx) => statx,
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    } else {
        let resolved = match resolve_path_at(dirfd, path.as_str()) {
            Ok(path) => path,
            Err(e) => return UserRet::from_error(e),
        };
        match active_impl::backend().metadata(resolved.as_str()) {
            Ok(meta) => fill_linux_statx(&meta, meta.size, mask),
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    };

    match copy_to_user_struct(statx_ptr, &statx) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}
