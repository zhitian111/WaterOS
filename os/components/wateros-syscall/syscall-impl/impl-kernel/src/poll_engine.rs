//! 共享 `poll` / `ppoll` / `pselect6` / `select` 就绪扫描与阻塞等待。

//! 本模块代码由AI完成
extern crate alloc;

use api_v0::ErrNo;
use api_v0::UserRet;
use ipc::signal::SignalSet;
use network::{stack, SocketKind, SocketState};
use platform::wall_clock;
use task::TaskTick;
use wateros_base_config::task::SCHED_TIMER_PERIOD_MS;

use crate::socket_fd;
use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

// 本变量代码由AI完成
pub(crate) const POLLIN : i16 = 0x001;
// 本变量代码由AI完成
pub(crate) const POLLOUT : i16 = 0x004;
pub(crate) const POLLPRI : i16 = 0x002;
pub(crate) const POLLRDHUP : i16 = 0x2000;
// 本变量代码由AI完成
pub(crate) const POLLERR : i16 = 0x008;
// 本变量代码由AI完成
pub(crate) const POLLHUP : i16 = 0x010;
// 本变量代码由AI完成
pub(crate) const POLLNVAL : i16 = 0x020;

// 本变量代码由AI完成
pub(crate) const FD_SETSIZE : usize = 1024;
const FD_SET_WORDS : usize = FD_SETSIZE / 64;
const SOCKET_READY_YIELD_SPINS : usize = 4;

#[repr(C)]
#[derive(Copy, Clone, Default)]
// 本结构代码由AI完成
pub(crate) struct PollFd {
    pub fd : i32,
    pub events : i16,
    pub revents : i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
// 本结构代码由AI完成
pub(crate) struct UserTimespec {
    pub sec : isize,
    pub nsec : isize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct UserTimeVal {
    pub sec : isize,
    pub usec : isize,
}

pub(crate) type FdSet = [u64; FD_SET_WORDS];

pub(crate) struct PollDeadline {
    expire_ns : Option<u128>,
}

/// 将纳秒时长向上取整为调度 tick。
pub(crate) fn ns_duration_to_ticks(total_ns : u128) -> u64 {
    if total_ns == 0 {
        return 0;
    }
    let tick_ns = (SCHED_TIMER_PERIOD_MS as u128).max(1) * 1_000_000;
    let ticks = total_ns.saturating_add(tick_ns - 1) / tick_ns;
    u64::try_from(ticks).unwrap_or(u64::MAX)
                        .max(1)
}

impl PollDeadline {
    fn now_ns() -> u128 {
        wall_clock::monotonic_ns().unwrap_or_else(|_| {
                                      (task::current_tick() as u128) *
                                      (SCHED_TIMER_PERIOD_MS as u128) *
                                      1_000_000
                                  })
    }

    pub(crate) fn infinite() -> Self { Self { expire_ns : None } }

    pub(crate) fn from_timespec_ptr(ptr : usize) -> Result<Self, ErrNo> {
        if ptr == 0 {
            return Ok(Self::infinite());
        }
        let ts = copy_from_user_struct::<UserTimespec>(ptr)?;
        Self::from_timespec(ts)
    }

    pub(crate) fn from_timespec(ts : UserTimespec) -> Result<Self, ErrNo> {
        if ts.sec < 0 || ts.nsec < 0 || ts.nsec >= 1_000_000_000 {
            return Err(ErrNo::EINVAL);
        }
        let duration_ns = (ts.sec as u128).saturating_mul(1_000_000_000)
                                          .saturating_add(ts.nsec as u128);
        Ok(Self { expire_ns : Some(Self::now_ns().saturating_add(duration_ns)) })
    }

    pub(crate) fn from_poll_millis(timeout_ms : isize) -> Result<Self, ErrNo> {
        if timeout_ms < 0 {
            return Ok(Self::infinite());
        }
        if timeout_ms == 0 {
            return Ok(Self { expire_ns : Some(Self::now_ns()) });
        }
        let timeout_ns = (timeout_ms as u128).saturating_mul(1_000_000);
        Ok(Self { expire_ns : Some(Self::now_ns().saturating_add(timeout_ns)) })
    }

    pub(crate) fn from_timeval_ptr(ptr : usize) -> Result<Self, ErrNo> {
        if ptr == 0 {
            return Ok(Self::infinite());
        }
        let tv = copy_from_user_struct::<UserTimeVal>(ptr)?;
        if tv.sec < 0 || tv.usec < 0 || tv.usec >= 1_000_000 {
            return Err(ErrNo::EINVAL);
        }
        let ts = UserTimespec { sec : tv.sec,
                                nsec : tv.usec * 1000 };
        Self::from_timespec(ts)
    }

    pub(crate) fn expired(&self) -> bool {
        match self.expire_ns {
            Some(exp) => Self::now_ns() >= exp,
            None => false,
        }
    }

    pub(crate) fn remaining_ticks(&self) -> u64 {
        match self.expire_ns {
            Some(exp) => ns_duration_to_ticks(exp.saturating_sub(Self::now_ns())),
            None => u64::MAX,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_ticks_round_up() {
        let tick_ns = (SCHED_TIMER_PERIOD_MS as u128) * 1_000_000;
        assert_eq!(ns_duration_to_ticks(0), 0);
        assert_eq!(ns_duration_to_ticks(1), 1);
        assert_eq!(ns_duration_to_ticks(tick_ns), 1);
        assert_eq!(ns_duration_to_ticks(tick_ns + 1), 2);
    }
}

struct ScanCtx {
    fds_ptr : usize,
    nfds : usize,
}

fn current_task_has_deliverable_signal() -> bool {
    task::current_task_id().is_some_and(|task_id| {
                               ipc::signal::has_deliverable(task_id).unwrap_or(false)
                           })
}

impl ScanCtx {
    fn scan_count(&self) -> Result<usize, ErrNo> {
        let (n, _) = scan_pollfds(self.fds_ptr, self.nfds)?;
        Ok(n)
    }
}

pub(crate) fn drive_network_stack() {
    match platform::timer::now_duration() {
        Ok(now) => {
            let millis = now.as_millis()
                            .min(i64::MAX as u128) as i64;
            stack::poll_at_millis(millis);
        }
        Err(_) => stack::poll(),
    }
    stack::poll_socket_events();
}

pub(crate) fn poll_socket_revents(fd : usize, events : i16) -> i16 {
    let Some(socket) = socket_fd::lookup(fd) else {
        return 0;
    };
    let mut revents = 0i16;
    let Ok(snapshot) = socket.poll_snapshot() else {
        return POLLNVAL;
    };

    match snapshot.kind {
        SocketKind::Tcp => match snapshot.state {
            SocketState::Listening { .. } => {
                if events & POLLIN != 0 && snapshot.has_pending_accept {
                    revents |= POLLIN;
                }
            }
            SocketState::Connecting => {
                // EINPROGRESS 只表示握手尚未完成，不能把等待状态误报成挂断。
                // 每轮 poll/epoll 扫描都会先驱动网络栈，状态变化后再在下面报告。
            }
            SocketState::Connected => {
                let peer_read_closed =
                    !snapshot.may_recv;
                if events & POLLRDHUP != 0 && peer_read_closed {
                    revents |= POLLRDHUP;
                }
                if events & POLLIN != 0 && (snapshot.can_recv || peer_read_closed) {
                    revents |= POLLIN;
                }
                if events & POLLOUT != 0 && snapshot.may_send && snapshot.send_capacity > 0 {
                    revents |= POLLOUT;
                }
                if !snapshot.is_connected {
                    revents |= POLLHUP;
                }
            }
            SocketState::Closed => {
                if events & POLLRDHUP != 0 {
                    revents |= POLLRDHUP;
                }
                if snapshot.connect_error.is_some() {
                    // connect 失败也属于一次“可写完成”；select 依赖 POLLOUT，
                    // poll/epoll 则通过 POLLERR 唤醒并继续读取 SO_ERROR。
                    if events & POLLOUT != 0 {
                        revents |= POLLOUT;
                    }
                    revents |= POLLERR;
                }
                revents |= POLLHUP;
            }
            _ => {}
        },
        SocketKind::Udp => {
            if events & POLLOUT != 0 {
                revents |= POLLOUT;
            }
            if events & POLLIN != 0 && snapshot.can_recv {
                revents |= POLLIN;
            }
        }
    }
    revents
}

// 本方法代码由AI完成
pub(crate) fn poll_revents_fd(fd : usize, events : i16) -> i16 {
    if socket_fd::lookup(fd).is_some() {
        return poll_socket_revents(fd, events);
    }
    match vfs::fd::with_current_io(fd, |handle| handle.poll_revents(events)) {
        Ok(r) => r,
        Err(_) => POLLNVAL,
    }
}

pub(crate) fn scan_pollfds(fds_ptr : usize, nfds : usize) -> Result<(usize, usize), ErrNo> {
    if nfds == 0 {
        return Ok((0, 0));
    }
    if fds_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if nfds > 1024 {
        return Err(ErrNo::EINVAL);
    }

    let pollfd_size = core::mem::size_of::<PollFd>();
    let mut ready_count = 0usize;
    let mut network_driven = false;

    for i in 0..nfds {
        let ptr = fds_ptr + i * pollfd_size;
        let mut pfd : PollFd = copy_from_user_struct(ptr)?;
        pfd.revents = 0;

        if pfd.fd < 0 {
            continue;
        }

        let fd = pfd.fd as usize;
        if !network_driven && socket_fd::lookup(fd).is_some() {
            drive_network_stack();
            network_driven = true;
        }
        let revents = poll_revents_fd(fd, pfd.events);
        if revents & POLLNVAL != 0 {
            pfd.revents = POLLNVAL;
            ready_count += 1;
            copy_to_user_struct(ptr, &pfd)?;
            continue;
        }
        pfd.revents = revents & (pfd.events | POLLHUP | POLLERR | POLLPRI);
        if pfd.revents != 0 {
            ready_count += 1;
            copy_to_user_struct(ptr, &pfd)?;
        }
    }

    Ok((ready_count, nfds))
}

fn poll_wait_pipe_fds(fds_ptr : usize,
                      nfds : usize,
                      deadline : &PollDeadline,
                      still_waiting : &mut dyn FnMut() -> bool)
                      -> Result<bool, ErrNo> {
    let pollfd_size = core::mem::size_of::<PollFd>();
    let remaining = deadline.remaining_ticks();
    if remaining == 0 {
        return Ok(false);
    }
    let wait_ticks = remaining.min(1);
    let mut any_pipe = false;
    for i in 0..nfds {
        if !still_waiting() {
            return Ok(true);
        }
        let ptr = fds_ptr + i * pollfd_size;
        let pfd : PollFd = copy_from_user_struct(ptr)?;
        if pfd.fd < 0 {
            continue;
        }
        let fd = pfd.fd as usize;
        if socket_fd::lookup(fd).is_some() {
            continue;
        }
        // 等待过程会主动切换任务，必须使用独立临时句柄，不能持有共享 fd
        // 槽锁睡眠；否则同进程线程访问该 fd 时会在单核上永久自旋。
        let mut wait_on_this_fd = || !deadline.expired();
        match vfs::fd::with_current_io_detached(fd, |handle| {
                  handle.poll_wait_for_ticks(pfd.events,
                                             wait_ticks,
                                             &mut wait_on_this_fd)
              }) {
            Ok(()) => any_pipe = true,
            Err(vfs::api::VfsError::Interrupted) => return Err(ErrNo::EINTR),
            Err(_) => {}
        }
    }
    Ok(any_pipe)
}

pub(crate) fn poll_block_until_ready(fds_ptr : usize,
                                     nfds : usize,
                                     deadline : PollDeadline)
                                     -> Result<usize, ErrNo> {
    let ctx = ScanCtx { fds_ptr, nfds };
    loop {
        let n = ctx.scan_count()?;
        if n > 0 {
            return Ok(n);
        }
        // A signal can arrive while this task is still running, immediately
        // before it enters the one-tick sleep. In that window interrupt_task()
        // cannot remove it from a wait queue, so every loop must also observe
        // the pending signal explicitly.
        if current_task_has_deliverable_signal() {
            return Err(ErrNo::EINTR);
        }
        if deadline.expired() {
            return Ok(0);
        }
        let remaining = deadline.remaining_ticks();
        if remaining == 0 {
            return Ok(0);
        }

        let mut still_waiting = || -> bool {
            ctx.scan_count()
               .unwrap_or(0) ==
            0 &&
            !deadline.expired()
        };
        let any_pipe = poll_wait_pipe_fds(fds_ptr,
                                          nfds,
                                          &deadline,
                                          &mut still_waiting)?;
        if !any_pipe {
            if task::sleep_for_ticks(1.min(remaining) as TaskTick) ==
               task::TaskWaitResult::Interrupted
            {
                return Err(ErrNo::EINTR);
            }
        }
    }
}

// 本方法代码由AI完成
pub(crate) fn do_poll_with_deadline(fds_ptr : usize,
                                    nfds : usize,
                                    deadline : PollDeadline)
                                    -> UserRet {
    let (n, _) = match scan_pollfds(fds_ptr, nfds) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };
    if n > 0 || deadline.expired() {
        return UserRet::from_success(n);
    }
    if deadline.remaining_ticks() == 0 {
        return UserRet::from_success(0);
    }
    match poll_block_until_ready(fds_ptr, nfds, deadline) {
        Ok(n) => UserRet::from_success(n),
        Err(e) => UserRet::from_error(e),
    }
}

pub(crate) fn validate_sigmask(sigmask_ptr : usize, sigsetsize : usize) -> Result<(), ErrNo> {
    if sigmask_ptr == 0 {
        return Ok(());
    }
    if sigsetsize != 8 {
        return Err(ErrNo::EINVAL);
    }
    Ok(())
}

/// `ppoll` / `pselect6` 期间临时替换线程信号掩码。
///
/// 正常完成时立即恢复；若 syscall 因当前临时 mask 下的信号返回 EINTR，则把恢复
/// 延迟到 signal frame/sigreturn，保证该信号仍可被真正投递。
pub(crate) struct PollSigmaskGuard {
    task_id : usize,
    active : bool,
}

impl PollSigmaskGuard {
    pub(crate) fn finish(mut self, interrupted : bool) {
        let defer_to_signal_frame =
            interrupted && ipc::signal::has_deliverable(self.task_id).unwrap_or(false);
        if !defer_to_signal_frame {
            let _ = ipc::signal::end_poll_sigmask(self.task_id);
        }
        self.active = false;
    }
}

impl Drop for PollSigmaskGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = ipc::signal::end_poll_sigmask(self.task_id);
        }
    }
}

pub(crate) fn install_poll_sigmask(sigmask_ptr : usize,
                                   sigsetsize : usize)
                                   -> Result<Option<PollSigmaskGuard>, ErrNo> {
    if sigmask_ptr == 0 {
        return Ok(None);
    }
    validate_sigmask(sigmask_ptr, sigsetsize)?;
    let bits = copy_from_user_struct::<u64>(sigmask_ptr)?;
    let task_id = task::current_task_id().ok_or(ErrNo::ESRCH)?;
    ipc::signal::begin_poll_sigmask(task_id, SignalSet::from_bits(bits)).map_err(|_| {
                                                                            ErrNo::EINVAL
                                                                        })?;
    Ok(Some(PollSigmaskGuard { task_id,
                               active:
                                   true }))
}

pub(crate) fn fd_set_get(set : &FdSet, fd : usize) -> bool {
    if fd >= FD_SETSIZE {
        return false;
    }
    let word = fd / 64;
    let bit = fd % 64;
    (set[word] >> bit) & 1 != 0
}

pub(crate) fn fd_set_set(set : &mut FdSet, fd : usize) {
    if fd >= FD_SETSIZE {
        return;
    }
    let word = fd / 64;
    let bit = fd % 64;
    set[word] |= 1u64 << bit;
}

pub(crate) fn copy_fd_set_from_user(ptr : usize) -> Result<Option<FdSet>, ErrNo> {
    if ptr == 0 {
        return Ok(None);
    }
    let mut set = [0u64; FD_SET_WORDS];
    let bytes = core::mem::size_of::<FdSet>();
    let slice = unsafe { core::slice::from_raw_parts_mut(set.as_mut_ptr() as *mut u8, bytes) };
    match crate::user_copy::copy_from_user(slice, ptr) {
        Ok(n) if n == bytes => Ok(Some(set)),
        _ => Err(ErrNo::EFAULT),
    }
}

pub(crate) fn copy_fd_set_to_user(ptr : usize, set : &FdSet) -> Result<(), ErrNo> {
    if ptr == 0 {
        return Ok(());
    }
    let bytes = core::mem::size_of::<FdSet>();
    let slice = unsafe { core::slice::from_raw_parts(set.as_ptr() as *const u8, bytes) };
    match crate::user_copy::copy_to_user(ptr, slice) {
        Ok(n) if n == bytes => Ok(()),
        _ => Err(ErrNo::EFAULT),
    }
}

fn scan_fd_sets_inner(nfds : usize,
                      readfds_ptr : usize,
                      writefds_ptr : usize,
                      exceptfds_ptr : usize,
                      writeback : bool)
                      -> Result<usize, ErrNo> {
    if nfds > FD_SETSIZE {
        return Err(ErrNo::EINVAL);
    }
    let read_in = copy_fd_set_from_user(readfds_ptr)?;
    let write_in = copy_fd_set_from_user(writefds_ptr)?;
    let except_in = copy_fd_set_from_user(exceptfds_ptr)?;

    let mut read_out = [0u64; FD_SET_WORDS];
    let mut write_out = [0u64; FD_SET_WORDS];
    let mut except_out = [0u64; FD_SET_WORDS];
    let mut ready_count = 0usize;
    let mut network_driven = false;

    for fd in 0..nfds {
        let mut events = 0i16;
        if read_in.as_ref()
                  .is_some_and(|s| fd_set_get(s, fd))
        {
            events |= POLLIN;
        }
        if write_in.as_ref()
                   .is_some_and(|s| fd_set_get(s, fd))
        {
            events |= POLLOUT;
        }
        if except_in.as_ref()
                    .is_some_and(|s| fd_set_get(s, fd))
        {
            events |= POLLPRI;
        }
        if events == 0 {
            continue;
        }
        if !network_driven && socket_fd::lookup(fd).is_some() {
            drive_network_stack();
            network_driven = true;
        }
        let revents = poll_revents_fd(fd, events);
        if revents == 0 {
            continue;
        }
        ready_count += 1;
        if revents & POLLIN != 0 && read_in.is_some() {
            fd_set_set(&mut read_out, fd);
        }
        if revents & POLLOUT != 0 && write_in.is_some() {
            fd_set_set(&mut write_out, fd);
        }
        if revents & (POLLPRI | POLLERR | POLLHUP) != 0 && except_in.is_some() {
            fd_set_set(&mut except_out, fd);
        }
    }

    if writeback && read_in.is_some() {
        copy_fd_set_to_user(readfds_ptr, &read_out)?;
    }
    if writeback && write_in.is_some() {
        copy_fd_set_to_user(writefds_ptr, &write_out)?;
    }
    if writeback && except_in.is_some() {
        copy_fd_set_to_user(exceptfds_ptr, &except_out)?;
    }

    Ok(ready_count)
}

fn fd_monitored_in_sets(fd : usize,
                        readfds_ptr : usize,
                        writefds_ptr : usize,
                        exceptfds_ptr : usize)
                        -> Result<i16, ErrNo> {
    let read_in = copy_fd_set_from_user(readfds_ptr)?;
    let write_in = copy_fd_set_from_user(writefds_ptr)?;
    let except_in = copy_fd_set_from_user(exceptfds_ptr)?;
    let mut events = 0i16;
    if read_in.as_ref()
              .is_some_and(|s| fd_set_get(s, fd))
    {
        events |= POLLIN;
    }
    if write_in.as_ref()
               .is_some_and(|s| fd_set_get(s, fd))
    {
        events |= POLLOUT;
    }
    if except_in.as_ref()
                .is_some_and(|s| fd_set_get(s, fd))
    {
        events |= POLLPRI;
    }
    Ok(events)
}

fn poll_wait_monitored_fds(nfds : usize,
                           readfds_ptr : usize,
                           writefds_ptr : usize,
                           exceptfds_ptr : usize,
                           deadline : &PollDeadline,
                           still_waiting : &mut dyn FnMut() -> bool)
                           -> Result<bool, ErrNo> {
    let remaining = deadline.remaining_ticks();
    if remaining == 0 {
        return Ok(false);
    }
    let wait_ticks = remaining.min(1);
    let mut any_pipe = false;
    for fd in 0..nfds {
        if !still_waiting() {
            return Ok(true);
        }
        let events = fd_monitored_in_sets(fd,
                                          readfds_ptr,
                                          writefds_ptr,
                                          exceptfds_ptr)?;
        if events == 0 {
            continue;
        }
        if socket_fd::lookup(fd).is_some() {
            continue;
        }
        // 同 `poll_wait_pipe_fds`：等待时不能占用共享 fd 槽锁。
        let mut wait_on_this_fd = || !deadline.expired();
        match vfs::fd::with_current_io_detached(fd, |handle| {
                  handle.poll_wait_for_ticks(events, wait_ticks, &mut wait_on_this_fd)
              }) {
            Ok(()) => any_pipe = true,
            Err(vfs::api::VfsError::Interrupted) => return Err(ErrNo::EINTR),
            Err(_) => {}
        }
    }
    Ok(any_pipe)
}

pub(crate) fn poll_block_fd_sets(nfds : usize,
                                 readfds_ptr : usize,
                                 writefds_ptr : usize,
                                 exceptfds_ptr : usize,
                                 deadline : PollDeadline)
                                 -> Result<usize, ErrNo> {
    loop {
        let n = scan_fd_sets_inner(nfds,
                                   readfds_ptr,
                                   writefds_ptr,
                                   exceptfds_ptr,
                                   false)?;
        if n > 0 {
            return scan_fd_sets_inner(nfds,
                                      readfds_ptr,
                                      writefds_ptr,
                                      exceptfds_ptr,
                                      true);
        }
        if current_task_has_deliverable_signal() {
            return Err(ErrNo::EINTR);
        }
        if deadline.expired() {
            scan_fd_sets_inner(nfds,
                               readfds_ptr,
                               writefds_ptr,
                               exceptfds_ptr,
                               true)?;
            return Ok(0);
        }
        let remaining = deadline.remaining_ticks();
        if remaining == 0 {
            return Ok(0);
        }
        let mut still_waiting = || -> bool {
            scan_fd_sets_inner(nfds,
                               readfds_ptr,
                               writefds_ptr,
                               exceptfds_ptr,
                               false).map(|c| c == 0)
                                     .unwrap_or(true) &&
            !deadline.expired()
        };
        let any_pipe = poll_wait_monitored_fds(nfds,
                                               readfds_ptr,
                                               writefds_ptr,
                                               exceptfds_ptr,
                                               &deadline,
                                               &mut still_waiting)?;
        if !any_pipe {
            let mut became_ready = false;
            for _ in 0..SOCKET_READY_YIELD_SPINS {
                task::yield_now();
                if scan_fd_sets_inner(nfds,
                                      readfds_ptr,
                                      writefds_ptr,
                                      exceptfds_ptr,
                                      false)? >
                   0
                {
                    became_ready = true;
                    break;
                }
                if deadline.expired() {
                    break;
                }
            }
            if became_ready {
                continue;
            }
            if task::sleep_for_ticks(1.min(remaining) as TaskTick) ==
               task::TaskWaitResult::Interrupted
            {
                return Err(ErrNo::EINTR);
            }
        }
    }
}

// 本方法代码由AI完成
pub(crate) fn do_pselect_with_deadline(nfds : usize,
                                       readfds_ptr : usize,
                                       writefds_ptr : usize,
                                       exceptfds_ptr : usize,
                                       deadline : PollDeadline)
                                       -> UserRet {
    let n = match scan_fd_sets_inner(nfds,
                                     readfds_ptr,
                                     writefds_ptr,
                                     exceptfds_ptr,
                                     false)
    {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };
    if n > 0 {
        return match scan_fd_sets_inner(nfds,
                                        readfds_ptr,
                                        writefds_ptr,
                                        exceptfds_ptr,
                                        true)
        {
            Ok(n) => UserRet::from_success(n),
            Err(e) => UserRet::from_error(e),
        };
    }
    if deadline.expired() {
        return match scan_fd_sets_inner(nfds,
                                        readfds_ptr,
                                        writefds_ptr,
                                        exceptfds_ptr,
                                        true)
        {
            Ok(_) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        };
    }
    if deadline.remaining_ticks() == 0 {
        return UserRet::from_success(0);
    }
    match poll_block_fd_sets(nfds,
                             readfds_ptr,
                             writefds_ptr,
                             exceptfds_ptr,
                             deadline)
    {
        Ok(v) => UserRet::from_success(v),
        Err(e) => UserRet::from_error(e),
    }
}
