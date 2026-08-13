//! 文件描述符关闭操作：`close(2)`、`close_range(2)`。

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

use crate::epoll_fd;
use crate::vfs_util::vfs_error_to_errno;

const CLOSE_RANGE_UNSHARE: usize = 1 << 1;
const CLOSE_RANGE_CLOEXEC: usize = 1 << 2;

/// PTY 端点已经全部释放后再投递挂断事件，避免在 TTY/VFS 锁内进入信号路径。
pub(crate) fn dispatch_terminal_events(terminal_ids : &[tty::TerminalId]) {
    for id in terminal_ids {
        for event in tty::take_control_events(*id) {
            crate::sys::ipc::signal::send_kernel_signal_to_process_group(
                task::ProcessId::from_raw(event.process_group), event.signal);
        }
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_close(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let pty = vfs::fd::current_pty_endpoint(fd).ok().flatten();
    let was_unix = crate::unix_sock::is_unix_fd(fd);
    let was_epoll = epoll_fd::is_epoll_fd(fd);
    let result = vfs::fd::close_fd(fd);
    let pty_id = pty.as_ref().map(tty::PtyEndpointHandle::id);
    drop(pty);
    if let Some(id) = pty_id {
        dispatch_terminal_events(core::slice::from_ref(&id));
    }
    if was_unix {
        if let Ok(task_id) = vfs::fd::current_task_id() {
            crate::unix_sock::unregister(task_id, fd);
        }
    }
    if was_epoll {
        epoll_fd::remove(fd);
    }
    match result {
        Ok(()) => UserRet::from_success(0),
        Err(err) => UserRet::from_error(vfs_error_to_errno(err)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_close_range(args: SyscallArgs) -> UserRet {
    let first = args.arg(0);
    let last = args.arg(1);
    let flags = args.arg(2);

    if flags & !(CLOSE_RANGE_UNSHARE | CLOSE_RANGE_CLOEXEC) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    // Linux 语义：先按需 unshare 出私有 fd 表，再执行区间关闭/CLOEXEC，
    // 保证与共享 fd 表的兄弟线程互不影响。
    if flags & CLOSE_RANGE_UNSHARE != 0 {
        if let Err(e) = vfs::fd::unshare_fd_table() {
            return UserRet::from_error(vfs_error_to_errno(e));
        }
    }
    if first > last {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if flags & CLOSE_RANGE_CLOEXEC != 0 {
        return match vfs::fd::set_fd_range_cloexec(first, last, true) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    match vfs::fd::close_fd_range(first, last) {
        Ok((closed_fds, terminal_ids)) => {
            for fd in closed_fds {
                if let Ok(task_id) = vfs::fd::current_task_id() {
                    crate::unix_sock::unregister(task_id, fd);
                }
            }
            dispatch_terminal_events(&terminal_ids);
            UserRet::from_success(0)
        }
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
