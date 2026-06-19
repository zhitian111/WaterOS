//! `openat(2)`：经 VFS 打开 ext4 根卷文件并分配 fd。

extern crate alloc;

use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsOpenFlags, VfsOpenOps};

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::{linux_open_flags_to_vfs, vfs_error_to_errno};

const O_ACCMODE: u32 = 3;
const O_RDWR: u32 = 2;
const O_CLOEXEC: u32 = 0o2000000;
const O_TMPFILE: u32 = 0o20200000;
const FD_CLOEXEC: usize = 1;

static NEXT_TMPFILE_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn sys_openat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let flags = args.arg(2) as u32;
    let _mode = args.arg(3);

    let path = match copy_user_path_cstr(path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    if flags & O_TMPFILE == O_TMPFILE {
        return open_tmpfile(resolved.as_str(), flags);
    }

    let vf = linux_open_flags_to_vfs(flags);

    let backend = active_impl::backend();
    let handle = match backend.open(resolved.as_str(), vf) {
        Ok(h) => h,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };

    match vfs::fd::alloc_fd(handle) {
        Ok(fd) => {
            if flags & O_CLOEXEC != 0 {
                if let Err(e) = vfs::fd::set_fd_flags(fd, FD_CLOEXEC) {
                    return UserRet::from_error(vfs_error_to_errno(e));
                }
            }
            UserRet::from_success(fd)
        }
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn open_tmpfile(dir_path: &str, flags: u32) -> UserRet {
    if flags & O_ACCMODE != O_RDWR {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let backend = active_impl::backend();
    match backend.metadata(dir_path) {
        Ok(meta) if meta.node_type == vfs::api::VfsNodeType::Directory => {}
        Ok(_) => return UserRet::from_error(ErrNo::ENOTDIR),
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    }

    let vf = VfsOpenFlags(
        VfsOpenFlags::READ | VfsOpenFlags::WRITE | VfsOpenFlags::CREATE | VfsOpenFlags::TRUNC,
    );
    let task_id = task::current_task_id().unwrap_or(0);
    for _ in 0..64 {
        let id = NEXT_TMPFILE_ID.fetch_add(1, Ordering::Relaxed);
        let tmp_path = if dir_path == "/" {
            format!("/.wateros-tmpfile-{task_id}-{id}")
        } else {
            format!("{}/.wateros-tmpfile-{task_id}-{id}", dir_path.trim_end_matches('/'))
        };
        let handle = match backend.open(tmp_path.as_str(), vf) {
            Ok(handle) => handle,
            Err(VfsError::Exists) => continue,
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        };
        return match vfs::fd::alloc_fd(handle) {
            Ok(fd) => {
                if flags & O_CLOEXEC != 0 {
                    if let Err(e) = vfs::fd::set_fd_flags(fd, FD_CLOEXEC) {
                        return UserRet::from_error(vfs_error_to_errno(e));
                    }
                }
                UserRet::from_success(fd)
            }
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    UserRet::from_error(ErrNo::EEXIST)
}
