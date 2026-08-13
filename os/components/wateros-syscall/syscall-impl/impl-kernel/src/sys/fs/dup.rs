//! `dup`/`dup3` 系统调用实现。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

use crate::epoll_fd;
use crate::vfs_util::vfs_error_to_errno;

/// Linux `O_CLOEXEC`（`dup3` flags）。
const O_CLOEXEC: usize = 0o2000000;

/// `dup(oldfd)` — 复制 fd 到最低可用编号。
// 本方法代码由AI完成
pub(crate) fn sys_dup(args: SyscallArgs) -> UserRet {
    let oldfd = args.arg(0);
    let epoll = epoll_fd::lookup(oldfd);
    let task_id = vfs::fd::current_task_id().ok();
    match vfs::fd::dup_fd(oldfd, 0) {
        Ok(newfd) => {
            if let Some(task_id) = task_id {
                crate::unix_sock::duplicate_registration(task_id, oldfd, newfd);
            }
            if let Some(instance) = epoll {
                epoll_fd::register(newfd, instance);
            }
            UserRet::from_success(newfd)
        }
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

/// `dup3(oldfd, newfd, flags)` — 复制 fd 到指定编号。
// 本方法代码由AI完成
pub(crate) fn sys_dup3(args: SyscallArgs) -> UserRet {
    let oldfd = args.arg(0);
    let newfd = args.arg(1);
    let flags = args.arg(2);
    if flags & !O_CLOEXEC != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if oldfd == newfd {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let cloexec = (flags & O_CLOEXEC) != 0;
    let epoll = epoll_fd::lookup(oldfd);
    let overwritten_epoll = epoll_fd::is_epoll_fd(newfd);
    // dup3 会原子关闭 newfd。先只记住 PTY ID；真正的挂断信号必须等
    // VFS 完成替换并释放旧打开文件描述后再投递。
    let overwritten_pty = vfs::fd::current_pty_endpoint(newfd).ok().flatten();
    let overwritten_pty_id = overwritten_pty.as_ref().map(tty::PtyEndpointHandle::id);
    let task_id = vfs::fd::current_task_id().ok();
    match vfs::fd::dup3_fd(oldfd, newfd, cloexec) {
        Ok(fd) => {
            drop(overwritten_pty);
            if let Some(id) = overwritten_pty_id {
                super::close::dispatch_terminal_events(core::slice::from_ref(&id));
            }
            if let Some(task_id) = task_id {
                crate::unix_sock::duplicate_registration(task_id, oldfd, fd);
            }
            if overwritten_epoll {
                epoll_fd::remove(newfd);
            }
            if let Some(instance) = epoll {
                epoll_fd::register(fd, instance);
            }
            UserRet::from_success(fd)
        }
        Err(e) => {
            drop(overwritten_pty);
            UserRet::from_error(vfs_error_to_errno(e))
        }
    }
}
