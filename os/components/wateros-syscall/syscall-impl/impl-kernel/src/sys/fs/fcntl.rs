//! `fcntl(2)` — 文件描述符控制。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use vfs::{VfsIoHandle, VfsSeekWhence};

use crate::socket_fd;
use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};
use crate::vfs_util::vfs_error_to_errno;
use vfs::fd::{self, Flock, F_RDLCK, F_UNLCK, F_WRLCK};

const F_DUPFD : usize = 0;
const F_GETFD : usize = 1;
const F_SETFD : usize = 2;
const F_GETFL : usize = 3;
const F_SETFL : usize = 4;
const F_GETLK : usize = 5;
const F_SETLK : usize = 6;
const F_SETLKW : usize = 7;
const F_DUPFD_CLOEXEC : usize = 1030;
const F_SETPIPE_SZ : usize = 1031;
const F_GETPIPE_SZ : usize = 1032;

const FD_CLOEXEC : usize = 1;
const O_RDWR : usize = 2;
const O_APPEND : usize = 0o0002000;
const O_NONBLOCK : usize = 0o0004000;
const O_DIRECT : usize = 0o00040000;
const F_SETFL_MASK : u32 = (O_APPEND | O_NONBLOCK | O_DIRECT) as u32;

const PAGE_SIZE : usize = 4096;
const MAX_PIPE_SIZE : usize = 1 << 20;

// 本方法代码由AI完成
pub(crate) fn sys_fcntl(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let cmd = args.arg(1);
    let arg = args.arg(2);

    let result = match cmd {
        F_DUPFD => fcntl_dupfd(fd, arg),
        F_GETFD => fcntl_getfd(fd),
        F_SETFD => fcntl_setfd(fd, arg),
        F_GETFL => fcntl_getfl(fd),
        F_SETFL => fcntl_setfl(fd, arg),
        F_DUPFD_CLOEXEC => fcntl_dupfd_cloexec(fd, arg),
        F_GETLK => fcntl_getlk(fd, arg),
        F_SETLK => fcntl_setlk(fd, arg, false),
        F_SETLKW => fcntl_setlk(fd, arg, true),
        F_GETPIPE_SZ => fcntl_getpipe_sz(fd),
        F_SETPIPE_SZ => fcntl_setpipe_sz(fd, arg),
        _ => fcntl_unknown_cmd(fd),
    };

    match result {
        Ok(n) => UserRet::from_success(n),
        Err(e) => UserRet::from_error(e),
    }
}

fn fcntl_unknown_cmd(fd : usize) -> Result<usize, ErrNo> {
    if vfs::fd::with_current_io(fd, |_| Ok(())).is_err() && socket_fd::lookup(fd).is_none() {
        return Err(ErrNo::EBADF);
    }
    Err(ErrNo::EINVAL)
}

fn fcntl_dupfd(fd : usize, minfd : usize) -> Result<usize, ErrNo> {
    let task_id = vfs::fd::current_task_id().map_err(vfs_error_to_errno)?;
    if minfd >= task::nofile_rlimit_for_task(task_id) as usize {
        return Err(ErrNo::EINVAL);
    }
    let epoll = crate::epoll_fd::lookup(fd);
    let newfd = vfs::fd::dup_fd(fd, minfd).map_err(vfs_error_to_errno)?;
    crate::unix_sock::duplicate_registration(task_id, fd, newfd);
    if let Some(instance) = epoll {
        crate::epoll_fd::register(newfd, instance);
    }
    Ok(newfd)
}

fn fcntl_dupfd_cloexec(fd : usize, minfd : usize) -> Result<usize, ErrNo> {
    let task_id = vfs::fd::current_task_id().map_err(vfs_error_to_errno)?;
    if minfd >= task::nofile_rlimit_for_task(task_id) as usize {
        return Err(ErrNo::EINVAL);
    }
    let epoll = crate::epoll_fd::lookup(fd);
    let newfd = vfs::fd::dup_fd(fd, minfd).map_err(vfs_error_to_errno)?;
    crate::unix_sock::duplicate_registration(task_id, fd, newfd);
    if let Some(instance) = epoll {
        crate::epoll_fd::register(newfd, instance);
    }
    if let Err(error) = vfs::fd::set_fd_flags(newfd, FD_CLOEXEC) {
        crate::epoll_fd::remove(newfd);
        let _ = vfs::fd::close_fd(newfd);
        return Err(vfs_error_to_errno(error));
    }
    Ok(newfd)
}

fn fcntl_getfd(fd : usize) -> Result<usize, ErrNo> {
    vfs::fd::get_fd_flags(fd).map_err(vfs_error_to_errno)
}

fn fcntl_setfd(fd : usize, arg : usize) -> Result<usize, ErrNo> {
    if arg & !FD_CLOEXEC != 0 {
        return Err(ErrNo::EINVAL);
    }
    vfs::fd::set_fd_flags(fd, arg).map_err(vfs_error_to_errno)?;
    Ok(0)
}

fn fcntl_getfl(fd : usize) -> Result<usize, ErrNo> {
    if let Some(flags) = socket_fd::status_flags(fd) {
        return Ok(O_RDWR | (flags & O_NONBLOCK));
    }
    vfs::fd::with_current_io(fd, |handle| {
        Ok(handle.open_accmode() as usize | handle.open_status_flags() as usize)
    }).map_err(vfs_error_to_errno)
}

fn fcntl_setfl(fd : usize, arg : usize) -> Result<usize, ErrNo> {
    if socket_fd::lookup(fd).is_some() {
        let flags = arg & O_NONBLOCK;
        socket_fd::set_status_flags(fd, flags).ok_or(ErrNo::EBADF)?;
        return Ok(0);
    }
    vfs::fd::with_current_io(fd, |handle| {
        handle.set_open_status_flags((arg as u32) & F_SETFL_MASK)
    }).map_err(vfs_error_to_errno)?;
    Ok(0)
}

fn current_pid() -> Result<task::ProcessId, ErrNo> {
    task::current_process_task_snapshot().map(|snap| snap.pid)
                                         .ok_or(ErrNo::ESRCH)
}

fn lockable_inode(fd : usize) -> Result<fd::InodeKey, ErrNo> {
    vfs::fd::with_current_io(fd, |handle| {
        let meta = handle.metadata()?;
        fd::inode_key_from_metadata(&meta).ok_or(vfs::api::VfsError::Unsupported)
    }).map_err(|err| match err {
          vfs::api::VfsError::Unsupported => ErrNo::EINVAL,
          other => vfs_error_to_errno(other),
      })
}

fn resolve_lock_start(handle : &mut (dyn VfsIoHandle + '_),
                      flock : &Flock)
                      -> vfs::api::VfsResult<u64> {
    match flock.l_whence {
        0 => Ok(flock.l_start as u64),
        1 => {
            let cur = handle.seek(0, VfsSeekWhence::Cur)?;
            Ok((cur as i64).saturating_add(flock.l_start) as u64)
        }
        2 => {
            let size = handle.metadata()?
                             .size;
            Ok((size as i64).saturating_add(flock.l_start) as u64)
        }
        _ => Err(vfs::api::VfsError::Unsupported),
    }
}

fn normalize_pipe_size(size : usize) -> Result<usize, ErrNo> {
    if size > MAX_PIPE_SIZE {
        return Err(ErrNo::EPERM);
    }
    let pages = size.max(1)
                    .div_ceil(PAGE_SIZE);
    let rounded_pages = pages.checked_next_power_of_two().ok_or(ErrNo::EPERM)?;
    let capacity = rounded_pages.checked_mul(PAGE_SIZE).ok_or(ErrNo::EPERM)?;
    if capacity > MAX_PIPE_SIZE {
        return Err(ErrNo::EPERM);
    }
    Ok(capacity)
}

fn fcntl_getpipe_sz(fd : usize) -> Result<usize, ErrNo> {
    vfs::fd::with_current_io(fd, |handle| {
        handle.pipe_capacity()
              .ok_or(vfs::api::VfsError::Unsupported)
    }).map_err(|err| match err {
          vfs::api::VfsError::Unsupported => ErrNo::EINVAL,
          other => vfs_error_to_errno(other),
      })
}

fn fcntl_setpipe_sz(fd : usize, arg : usize) -> Result<usize, ErrNo> {
    let size = normalize_pipe_size(arg)?;
    vfs::fd::with_current_io(fd, |handle| {
        handle.pipe_set_capacity(size)
    }).map_err(|err| match err {
          vfs::api::VfsError::Unsupported => ErrNo::EINVAL,
          other => vfs_error_to_errno(other),
      })
}

fn fcntl_getlk(fd : usize, flock_ptr : usize) -> Result<usize, ErrNo> {
    let mut flock = copy_from_user_struct::<Flock>(flock_ptr)?;
    if flock.l_type != F_RDLCK && flock.l_type != F_WRLCK {
        return Err(ErrNo::EINVAL);
    }
    let key = lockable_inode(fd)?;
    let pid = current_pid()?;

    let resolved_start = vfs::fd::with_current_io(fd, |handle| {
                             resolve_lock_start(handle, &flock)
                         }).map_err(vfs_error_to_errno)?;
    flock.l_start = resolved_start as i64;

    fd::posix_getlk(&key, pid, &mut flock).map_err(vfs_error_to_errno)?;
    copy_to_user_struct(flock_ptr, &flock)?;
    Ok(0)
}

fn fcntl_setlk(fd : usize, flock_ptr : usize, blocking : bool) -> Result<usize, ErrNo> {
    let mut flock = copy_from_user_struct::<Flock>(flock_ptr)?;
    if flock.l_type != F_RDLCK && flock.l_type != F_WRLCK && flock.l_type != F_UNLCK {
        return Err(ErrNo::EINVAL);
    }
    let key = lockable_inode(fd)?;
    let pid = current_pid()?;

    if flock.l_type != F_UNLCK {
        let resolved_start = vfs::fd::with_current_io(fd, |handle| {
                                 resolve_lock_start(handle, &flock)
                             }).map_err(vfs_error_to_errno)?;
        flock.l_start = resolved_start as i64;
    }

    fd::posix_setlk(&key, pid, &flock, blocking).map_err(vfs_error_to_errno)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_size_rounds_to_page_power_of_two() {
        assert_eq!(normalize_pipe_size(0), Ok(PAGE_SIZE));
        assert_eq!(normalize_pipe_size(1), Ok(PAGE_SIZE));
        assert_eq!(normalize_pipe_size(PAGE_SIZE), Ok(PAGE_SIZE));
        assert_eq!(normalize_pipe_size(PAGE_SIZE + 1), Ok(PAGE_SIZE * 2));
        assert_eq!(normalize_pipe_size(PAGE_SIZE * 3), Ok(PAGE_SIZE * 4));
    }

    #[test]
    fn pipe_size_rejects_over_limit() {
        assert_eq!(normalize_pipe_size(MAX_PIPE_SIZE + 1), Err(ErrNo::EPERM));
    }
}
