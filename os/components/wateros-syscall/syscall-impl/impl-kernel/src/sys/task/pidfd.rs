//! pidfd 进程句柄：稳定引用 PID，并接入 poll、信号和跨进程 fd 复制。

use alloc::boxed::Box;

use api_v0::{ErrNo, SyscallArgs, UserRet};
use task::{ProcessId, ProcessState};
use vfs::api::{VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsResult};

use crate::vfs_util::vfs_error_to_errno;

const PIDFD_NONBLOCK : usize = 0x800;
const POLLIN : i16 = 0x0001;
const POLLRDNORM : i16 = 0x0040;
const POLLHUP : i16 = 0x0010;

/// pidfd 只保存带 generation 的 WaterOS `ProcessId`，不会把进程 registry 锁
/// 延长到 fd 生命周期。进程已被 reap 后，PID 不再可复用为同一个 ProcessId，
/// 句柄仍保持“已退出且可读”的稳定状态。
#[derive(Clone)]
pub(crate) struct PidFdHandle {
    pid : ProcessId,
    nonblocking : bool,
}

impl PidFdHandle {
    fn new(pid : ProcessId, nonblocking : bool) -> Self { Self { pid, nonblocking } }

    fn exited(&self) -> bool {
        task::process_snapshot(self.pid)
            .map(|snapshot| matches!(snapshot.state,
                                     ProcessState::Exited(_) | ProcessState::Exiting(_)))
            .unwrap_or(true)
    }

    fn ready(&self, events : i16) -> i16 {
        if !self.exited() {
            return 0;
        }
        (events & (POLLIN | POLLRDNORM)) | POLLHUP
    }
}

impl VfsIoHandle for PidFdHandle {
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(VfsMetadata { node_type : VfsNodeType::Special,
                         size : 0,
                         mode : 0o600,
                         device_major : 0,
                         device_minor : 0,
                         inode : self.pid.raw() as u64,
                         mount_id : 0,
                         nlink : 1,
                         uid : 0,
                         gid : 0 })
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> { Ok(Box::new(self.clone())) }

    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> { Ok(self.ready(events)) }

    fn poll_wait_for_ticks(&mut self,
                           events : i16,
                           timeout_ticks : u64,
                           still_waiting : &mut dyn FnMut() -> bool)
                           -> VfsResult<()> {
        if self.ready(events) != 0 || timeout_ticks == 0 || !still_waiting() {
            return Ok(());
        }
        // 进程 registry 与 fd 子系统不共享锁。按 poll engine 的单 tick 粒度
        // 睡眠后重扫，避免在句柄锁内挂到一个可能已被 reap 的 task-exit 队列。
        if task::sleep_for_ticks(timeout_ticks.min(1)) == task::TaskWaitResult::Interrupted {
            Err(VfsError::Interrupted)
        } else {
            Ok(())
        }
    }

    fn open_status_flags(&self) -> u32 {
        if self.nonblocking { PIDFD_NONBLOCK as u32 } else { 0 }
    }

    fn open_accmode(&self) -> u32 { 0 }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        self.nonblocking = flags as usize & PIDFD_NONBLOCK != 0;
        Ok(())
    }
}

pub(crate) fn pidfd_process_id(fd : usize) -> Result<ProcessId, ErrNo> {
    vfs::fd::with_current_io(fd, |handle| {
        handle.as_any()
              .downcast_ref::<PidFdHandle>()
              .map(|pidfd| pidfd.pid)
              .ok_or(VfsError::BadFd)
    }).map_err(vfs_error_to_errno)
}

fn allocate_pidfd(pid : ProcessId, nonblocking : bool) -> Result<usize, ErrNo> {
    let fd = vfs::fd::alloc_fd(Box::new(PidFdHandle::new(pid, nonblocking)))
        .map_err(vfs_error_to_errno)?;
    // Linux pidfd_open 总是返回 close-on-exec fd。
    if let Err(error) = vfs::fd::set_fd_flags(fd, 1) {
        let _ = vfs::fd::close_fd(fd);
        return Err(vfs_error_to_errno(error));
    }
    Ok(fd)
}

/// `pidfd_open(pid, flags)`。
pub(crate) fn sys_pidfd_open(args : SyscallArgs) -> UserRet {
    let pid_raw = args.arg(0) as i32;
    let flags = args.arg(1);
    if pid_raw <= 0 || flags & !PIDFD_NONBLOCK != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let pid = ProcessId::from_raw(pid_raw as usize);
    if task::process_snapshot(pid).is_none() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    match allocate_pidfd(pid, flags & PIDFD_NONBLOCK != 0) {
        Ok(fd) => UserRet::from_success(fd),
        Err(error) => UserRet::from_error(error),
    }
}

/// `pidfd_send_signal(pidfd, sig, info, flags)`。
pub(crate) fn sys_pidfd_send_signal(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let signal = args.arg(1) as i32;
    let info = args.arg(2);
    let flags = args.arg(3);
    if signal < 0 || signal > 64 || flags != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    // 非空 siginfo 需要完整 SI_QUEUE 凭据与 payload 语义；不能静默丢弃。
    if info != 0 {
        return UserRet::from_error(ErrNo::EOPNOTSUPP);
    }
    let pid = match pidfd_process_id(fd) {
        Ok(pid) => pid,
        Err(error) => return UserRet::from_error(error),
    };
    let result = if signal == 0 {
        crate::sys::ipc::signal::check_signal_permission(pid, 0)
    } else {
        crate::sys::ipc::signal::send_signal_to_process(pid, signal as usize)
    };
    match result {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

/// `pidfd_getfd(pidfd, targetfd, flags)`。
pub(crate) fn sys_pidfd_getfd(args : SyscallArgs) -> UserRet {
    let pidfd = args.arg(0);
    let target_fd = args.arg(1);
    if args.arg(2) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let pid = match pidfd_process_id(pidfd) {
        Ok(pid) => pid,
        Err(error) => return UserRet::from_error(error),
    };
    if let Err(error) = crate::sys::ipc::signal::check_signal_permission(pid, 0) {
        return UserRet::from_error(error);
    }
    let Some(task_id) = task::leader_task_for_process(pid) else {
        return UserRet::from_error(ErrNo::ESRCH);
    };
    let handle = match vfs::fd::duplicate_fd_from_task(task_id, target_fd) {
        Ok(handle) => handle,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    match vfs::fd::alloc_fd(handle) {
        Ok(fd) => UserRet::from_success(fd),
        Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
    }
}
