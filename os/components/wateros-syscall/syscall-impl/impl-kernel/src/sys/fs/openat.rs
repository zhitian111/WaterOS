//! `openat(2)`：经 VFS 打开 ext4 根卷文件并分配 fd。

//! 本模块代码由AI完成
use alloc::string::String;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{
    FinalSymlink, SingleRootReadView, VfsError, VfsMetadata, VfsNodeType, VfsOpenOps,
};

use super::path_at::{resolve_path_at, resolve_symlinks};
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::{linux_open_flags_to_vfs, vfs_error_to_errno};

const O_ACCMODE: u32 = 3;
const O_WRONLY: u32 = 1;
const O_RDWR: u32 = 2;
const O_CLOEXEC: u32 = 0o2000000;
const O_PATH: u32 = 0o10000000;
const O_NOCTTY: u32 = 0o400;
// asm-generic (and therefore RISC-V/LoongArch): O_LARGEFILE is 0100000,
// while O_NOFOLLOW is 0400000.  musl includes O_LARGEFILE in ordinary
// 64-bit opens, so conflating the two makes every shared-library symlink
// fail with ELOOP.
const O_NOFOLLOW: u32 = 0o400_000;
const O_EXCL: u32 = 0o200;
const O_CREAT: u32 = 0o100;
const O_TRUNC: u32 = 0o1000;
const FD_CLOEXEC: usize = 1;

const O_APPEND: u32 = 0o2000;
const O_NONBLOCK: u32 = 0o4000;
const O_LARGEFILE: u32 = 0o100_000;
const O_DIRECTORY: u32 = 0o200_000;
const SUPPORTED_OPEN_FLAGS: u32 = O_ACCMODE | O_CREAT | O_EXCL | O_NOCTTY | O_TRUNC |
                                  O_APPEND | O_NONBLOCK | O_LARGEFILE | O_DIRECTORY |
                                  O_NOFOLLOW | O_CLOEXEC | O_PATH;
const O_DSYNC: u32 = 0o10_000;
const O_ASYNC: u32 = 0o20_000;
const O_DIRECT: u32 = 0o40_000;
const O_NOATIME: u32 = 0o1_000_000;
const O_SYNC: u32 = 0o4_010_000;
const O_TMPFILE: u32 = 0o20_200_000;
const KNOWN_UNSUPPORTED_OPEN_FLAGS: u32 = O_DSYNC | O_ASYNC | O_DIRECT | O_NOATIME | O_SYNC;

// 本方法代码由AI完成
pub(crate) fn sys_openat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let flags = args.arg(2) as u32;
    let mode = (args.arg(3) as u32) & 0o7777;

    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    openat_path(dirfd, path.as_str(), flags, mode)
}

/// 已完成用户复制后的共用打开入口，供 `openat2` 在校验 `open_how` 后复用。
pub(crate) fn openat_path(dirfd : isize, path : &str, flags : u32, mode : u32) -> UserRet {
    if let Err(error) = validate_open_flags(flags) {
        return UserRet::from_error(error);
    }
    let resolved = match resolve_path_at(dirfd, path) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    open_resolved_path_unchecked(resolved.as_str(), flags, mode)
}

/// 打开已经由 syscall 路径约束解析器得到的物理绝对路径。
pub(crate) fn open_resolved_path(resolved : &str, flags : u32, mode : u32) -> UserRet {
    if let Err(error) = validate_open_flags(flags) {
        return UserRet::from_error(error);
    }
    open_resolved_path_unchecked(resolved, flags, mode)
}

fn validate_open_flags(flags : u32) -> Result<(), ErrNo> {
    if flags & O_ACCMODE == O_ACCMODE || flags & !(SUPPORTED_OPEN_FLAGS | O_TMPFILE |
                                                    KNOWN_UNSUPPORTED_OPEN_FLAGS) != 0
    {
        return Err(ErrNo::EINVAL);
    }
    if flags & KNOWN_UNSUPPORTED_OPEN_FLAGS != 0 {
        return Err(ErrNo::EOPNOTSUPP);
    }
    if flags & (O_TMPFILE & !O_DIRECTORY) != 0 && flags & O_TMPFILE != O_TMPFILE {
        return Err(ErrNo::EINVAL);
    }
    Ok(())
}

fn open_resolved_path_unchecked(resolved : &str, flags : u32, mode : u32) -> UserRet {
    if flags & O_TMPFILE == O_TMPFILE {
        if flags & (O_PATH | O_CREAT) != 0 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        let directory = match prepare_open_path(resolved, flags) {
            Ok(path) => path,
            Err(error) => return UserRet::from_error(error),
        };
        return open_tmpfile(directory.as_str(), flags, mode);
    }
    let open_path = match prepare_open_path(resolved, flags) {
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
            // UNIX98 PTY slave：session leader 在未指定 O_NOCTTY 时自动取得控制终端。
            // nxterm 使用 setsid()+open(ptsname) 而不会额外调用 TIOCSCTTY。
            if flags & O_NOCTTY == 0 {
                if let (Ok(Some(endpoint)), Some(process)) =
                    (vfs::fd::current_pty_endpoint(fd), task::current_process_snapshot())
                {
                    if endpoint.endpoint() == tty::TerminalEndpoint::PtySlave &&
                       process.pid == process.sid && endpoint.controlling_sid() == 0
                    {
                        let _ = tty::attach_session(&endpoint,
                                                    process.sid.raw(),
                                                    process.pgid.raw());
                    }
                }
            }
            if flags & O_CLOEXEC != 0 {
                if let Err(e) = vfs::fd::set_fd_flags(fd, FD_CLOEXEC) {
                    let _ = vfs::fd::close_fd(fd);
                    return UserRet::from_error(vfs_error_to_errno(e));
                }
            }
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
            let is_dir = active_impl::backend().metadata(open_path.as_str())
                                    .map(|meta| meta.node_type == VfsNodeType::Directory)
                                    .unwrap_or(false);
            if creates_new_file {
                super::inotify::notify_create(open_path.as_str(), false);
            } else {
                super::inotify::notify_open(open_path.as_str(), is_dir);
                if flags & O_TRUNC != 0 {
                    super::inotify::notify_modify(open_path.as_str());
                }
            }
            UserRet::from_success(fd)
        }
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn open_tmpfile(directory : &str, flags : u32, mode : u32) -> UserRet {
    if flags & O_ACCMODE == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let cred = cred::current_credentials();
    let parent = match super::dir::check_directory_create(directory, &cred) {
        Ok(parent) => parent,
        Err(error) => return UserRet::from_error(error),
    };
    let create_mode = mode & !super::super::task::current_umask();
    let gid = if parent.mode & 0o2000 != 0 {
        parent.gid
    } else {
        cred.effective_gid.0
    };
    let handle = match vfs::open_tmpfile_absolute(
        directory,
        linux_open_flags_to_vfs(flags),
        create_mode,
        cred.effective_uid.0,
        gid,
        flags & O_EXCL == 0,
    ) {
        Ok(handle) => handle,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    let fd = match vfs::fd::alloc_fd(handle) {
        Ok(fd) => fd,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    if flags & O_CLOEXEC != 0 {
        if let Err(error) = vfs::fd::set_fd_flags(fd, FD_CLOEXEC) {
            let _ = vfs::fd::close_fd(fd);
            return UserRet::from_error(vfs_error_to_errno(error));
        }
    }
    if flags & O_NONBLOCK != 0 {
        if let Err(error) = vfs::fd::with_current_io(fd, |handle| {
            let status = handle.open_status_flags() | O_NONBLOCK;
            handle.set_open_status_flags(status)
        }) {
            let _ = vfs::fd::close_fd(fd);
            return UserRet::from_error(vfs_error_to_errno(error));
        }
    }
    UserRet::from_success(fd)
}

fn prepare_open_path(resolved: &str, flags: u32) -> Result<String, ErrNo> {
    let final_mode = final_symlink_mode(flags);
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

fn final_symlink_mode(flags: u32) -> FinalSymlink {
    if flags & O_NOFOLLOW != 0 {
        FinalSymlink::NoFollow
    } else {
        FinalSymlink::Follow
    }
}

#[cfg(test)]
mod tests {
    use super::{final_symlink_mode, O_NOFOLLOW};
    use vfs::api::FinalSymlink;

    #[test]
    fn largefile_does_not_disable_final_symlink_following() {
        const O_LARGEFILE: u32 = 0o100_000;
        assert_eq!(final_symlink_mode(O_LARGEFILE), FinalSymlink::Follow);
    }

    #[test]
    fn nofollow_uses_asm_generic_flag_value() {
        assert_eq!(O_NOFOLLOW, 0o400_000);
        assert_eq!(final_symlink_mode(O_NOFOLLOW), FinalSymlink::NoFollow);
    }
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
