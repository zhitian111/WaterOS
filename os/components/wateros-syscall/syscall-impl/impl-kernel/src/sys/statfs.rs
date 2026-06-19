//! `statfs(2)`：bring-up 最小兼容实现。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::SingleRootReadView;

use crate::sys::path_at::{resolve_path_at, AT_FDCWD};
use crate::user_copy::{copy_to_user_struct, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;

const EXT4_SUPER_MAGIC: isize = 0xEF53;
const STATFS_BLOCK_SIZE: isize = 4096;
const STATFS_TOTAL_BLOCKS: isize = 1024 * 1024;
const STATFS_FREE_BLOCKS: isize = 512 * 1024;
const STATFS_TOTAL_FILES: isize = 1024 * 1024;
const STATFS_FREE_FILES: isize = 512 * 1024;
const STATFS_MAX_NAME_LEN: isize = 255;

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxStatFs {
    f_type: isize,
    f_bsize: isize,
    f_blocks: isize,
    f_bfree: isize,
    f_bavail: isize,
    f_files: isize,
    f_ffree: isize,
    f_fsid: [i32; 2],
    f_namelen: isize,
    f_frsize: isize,
    f_flags: isize,
    f_spare: [isize; 4],
}

const _: () = assert!(core::mem::size_of::<LinuxStatFs>() == 120);

pub(crate) fn sys_statfs(args: SyscallArgs) -> UserRet {
    let path_ptr = args.arg(0);
    let buf_ptr = args.arg(1);
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_path_at(AT_FDCWD, path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    match active_impl::backend().metadata(resolved.as_str()) {
        Ok(_) => {}
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    }

    let statfs = LinuxStatFs {
        f_type: EXT4_SUPER_MAGIC,
        f_bsize: STATFS_BLOCK_SIZE,
        f_blocks: STATFS_TOTAL_BLOCKS,
        f_bfree: STATFS_FREE_BLOCKS,
        f_bavail: STATFS_FREE_BLOCKS,
        f_files: STATFS_TOTAL_FILES,
        f_ffree: STATFS_FREE_FILES,
        f_fsid: [0; 2],
        f_namelen: STATFS_MAX_NAME_LEN,
        f_frsize: STATFS_BLOCK_SIZE,
        f_flags: 0,
        f_spare: [0; 4],
    };

    match copy_to_user_struct(buf_ptr, &statfs) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}
