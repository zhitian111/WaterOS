//! `openat(2)`：经 VFS 打开 ext4 根卷文件并分配 fd。

//! 本模块代码由AI完成
extern crate alloc;

use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicU64, Ordering};

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{
    FinalSymlink, SingleRootReadView, VfsError, VfsMetadata, VfsNodeType, VfsOpenFlags, VfsOpenOps,
};

use crate::sys::ltp_cgroup_helper::cgroup_regression_loop_fast_exit_if_standalone;
use super::path_at::{resolve_path_at, resolve_symlinks};
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::{linux_open_flags_to_vfs, vfs_error_to_errno};

const O_ACCMODE: u32 = 3;
const O_WRONLY: u32 = 1;
const O_RDWR: u32 = 2;
const O_CLOEXEC: u32 = 0o2000000;
const O_PATH: u32 = 0o10000000;
const O_TMPFILE: u32 = 0o20200000;
const O_NOFOLLOW: u32 = 0o100_000;
const O_EXCL: u32 = 0o200;
const O_CREAT: u32 = 0o100;
const O_TRUNC: u32 = 0o1000;
const FD_CLOEXEC: usize = 1;

static NEXT_TMPFILE_ID: AtomicU64 = AtomicU64::new(1);

// 本方法代码由AI完成
pub(crate) fn sys_openat(args : SyscallArgs) -> UserRet {
    cgroup_regression_loop_fast_exit_if_standalone();

    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let flags = args.arg(2) as u32;
    let mode = (args.arg(3) as u32) & 0o7777;

    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
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

    let open_path = match prepare_open_path(resolved.as_str(), flags) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    if flags & (O_CREAT | O_EXCL) == (O_CREAT | O_EXCL) {
        match active_impl::backend().metadata(open_path.as_str()) {
            Ok(_) => return UserRet::from_error(ErrNo::EEXIST),
            Err(VfsError::NotFound) => {}
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    }
    let creates_new_file = flags & O_CREAT != 0
        && matches!(
            active_impl::backend().metadata(open_path.as_str()),
            Err(VfsError::NotFound)
        );

    let vf = linux_open_flags_to_vfs(flags);
    if let Err(e) = check_existing_open_permission(open_path.as_str(), flags, creates_new_file) {
        return UserRet::from_error(e);
    }

    let backend = active_impl::backend();
    let handle = match backend.open(open_path.as_str(), vf) {
        Ok(h) => h,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };
    if creates_new_file {
        let cred = cred::current_credentials();
        if let Err(e) = vfs::chown_absolute(
            open_path.as_str(),
            Some(cred.effective_uid.0),
            Some(cred.effective_gid.0),
        ) {
            return UserRet::from_error(vfs_error_to_errno(e));
        }
        let create_mode = mode & !super::super::task::current_umask();
        if let Err(e) = vfs::chmod_absolute(open_path.as_str(), create_mode) {
            return UserRet::from_error(vfs_error_to_errno(e));
        }
    }

    match vfs::fd::alloc_fd(handle) {
        Ok(fd) => {
            if flags & O_CLOEXEC != 0 {
                if let Err(e) = vfs::fd::set_fd_flags(fd, FD_CLOEXEC) {
                    let _ = vfs::fd::close_fd(fd);
                    return UserRet::from_error(vfs_error_to_errno(e));
                }
            }
            const O_NONBLOCK: u32 = 0o4000;
            if flags & O_NONBLOCK != 0 {
                if let Err(e) = vfs::fd::with_current_io(fd, |h| {
                    let mut sf = h.open_status_flags();
                    sf |= O_NONBLOCK;
                    h.set_open_status_flags(sf)
                }) {
                    let _ = vfs::fd::close_fd(fd);
                    return UserRet::from_error(vfs_error_to_errno(e));
                }
            }
            if flags & O_PATH != 0 {
                if let Err(e) = vfs::fd::set_path_only_fd(fd) {
                    let _ = vfs::fd::close_fd(fd);
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
                        let _ = vfs::fd::close_fd(fd);
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

fn prepare_open_path(resolved: &str, flags: u32) -> Result<String, ErrNo> {
    let final_mode = if flags & O_NOFOLLOW != 0 {
        FinalSymlink::NoFollow
    } else {
        FinalSymlink::Follow
    };
    let resolved = match resolve_symlinks(resolved, final_mode) {
        Ok(path) => path,
        Err(ErrNo::ENOENT) if flags & O_CREAT != 0 => {
            resolve_symlinks(resolved, FinalSymlink::NoFollow)?
        }
        Err(error) => return Err(error),
    };

    if flags & O_NOFOLLOW != 0 {
        match active_impl::backend().metadata(resolved.as_str()) {
            Ok(meta) if meta.node_type == vfs::api::VfsNodeType::Symlink => {
                return Err(ErrNo::ELOOP);
            }
            Ok(_) => return Ok(resolved),
            Err(VfsError::NotFound) => return Ok(resolved),
            Err(e) => return Err(vfs_error_to_errno(e)),
        }
    }
    Ok(resolved)
}

fn check_existing_open_permission(path: &str, flags: u32, creates_new_file: bool) -> Result<(), ErrNo> {
    if creates_new_file || flags & O_PATH != 0 {
        return Ok(());
    }
    let meta = match active_impl::backend().metadata(path) {
        Ok(meta) => meta,
        Err(VfsError::NotFound) => return Ok(()),
        Err(e) => return Err(vfs_error_to_errno(e)),
    };
    let access_mode = flags & O_ACCMODE;
    let wants_read = access_mode == 0 || access_mode == O_RDWR;
    let wants_write = access_mode == O_WRONLY || access_mode == O_RDWR || flags & O_TRUNC != 0;
    if wants_write && meta.node_type == VfsNodeType::Directory {
        return Err(ErrNo::EISDIR);
    }
    if flags & O_CREAT != 0 && meta.node_type == VfsNodeType::Directory {
        return Err(ErrNo::EISDIR);
    }
    let cred = cred::current_credentials();
    if can_access_existing(&meta, &cred, wants_read, wants_write) {
        Ok(())
    } else {
        Err(ErrNo::EACCES)
    }
}

fn can_access_existing(
    meta: &VfsMetadata,
    cred: &ProcessCredentials,
    wants_read: bool,
    wants_write: bool,
) -> bool {
    if cred.effective_uid.0 == 0 {
        return true;
    }
    let mode = u32::from(meta.mode);
    let (read_bit, write_bit) = if cred.effective_uid.0 == meta.uid {
        (0o400, 0o200)
    } else if cred_has_group(cred, Gid(meta.gid)) {
        (0o040, 0o020)
    } else {
        (0o004, 0o002)
    };
    (!wants_read || mode & read_bit != 0) && (!wants_write || mode & write_bit != 0)
}

fn cred_has_group(cred: &ProcessCredentials, gid: Gid) -> bool {
    cred.effective_gid == gid
        || cred
            .supplementary_groups
            .iter()
            .take(cred.supplementary_group_len)
            .any(|group| *group == gid)
}
