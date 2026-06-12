//! 共享 `poll` / `ppoll` / `pselect6` / `select` 就绪扫描与阻塞等待。

extern crate alloc;

use abi::errno::ErrNo;
use abi::user_ret::UserRet;
use driver::network::stack;
use task::TaskTick;

use crate::socket_fd;
use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

pub(crate) const POLLIN: i16 = 0x001;
pub(crate) const POLLOUT: i16 = 0x004;
pub(crate) const POLLPRI: i16 = 0x002;
pub(crate) const POLLERR: i16 = 0x008;
pub(crate) const POLLHUP: i16 = 0x010;
pub(crate) const POLLNVAL: i16 = 0x020;

pub(crate) const FD_SETSIZE: usize = 1024;
const FD_SET_WORDS: usize = FD_SETSIZE / 64;

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub(crate) struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct UserTimespec {
    pub sec: isize,
    pub nsec: isize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct UserTimeVal {
    pub sec: isize,
    pub usec: isize,
}

pub(crate) type FdSet = [u64; FD_SET_WORDS];

pub(crate) struct PollDeadline {
    expire_tick: Option<u64>,
}

impl PollDeadline {
    pub(crate) fn infinite() -> Self {
        Self { expire_tick: None }
    }

    pub(crate) fn from_timespec_ptr(ptr: usize) -> Result<Self, ErrNo> {
        if ptr == 0 {
            return Ok(Self::infinite());
        }
        let ts = copy_from_user_struct::<UserTimespec>(ptr)?;
        Self::from_timespec(ts)
    }

    pub(crate) fn from_timespec(ts: UserTimespec) -> Result<Self, ErrNo> {
        if ts.sec < 0 || ts.nsec < 0 || ts.nsec >= 1_000_000_000 {
            return Err(ErrNo::EINVAL);
        }
        if ts.sec == 0 && ts.nsec == 0 {
            return Ok(Self {
                expire_tick: Some(task::current_tick()),
            });
        }
        let base = task::current_tick();
        let extra = if ts.sec > 0 || ts.nsec > 0 {
            1u64
        } else {
            0
        };
        Ok(Self {
            expire_tick: Some(base.saturating_add(extra)),
        })
    }

    pub(crate) fn from_poll_millis(timeout_ms: isize) -> Result<Self, ErrNo> {
        if timeout_ms < 0 {
            return Ok(Self::infinite());
        }
        if timeout_ms == 0 {
            return Ok(Self {
                expire_tick: Some(task::current_tick()),
            });
        }
        Ok(Self {
            expire_tick: Some(task::current_tick().saturating_add(1)),
        })
    }

    pub(crate) fn from_timeval_ptr(ptr: usize) -> Result<Self, ErrNo> {
        if ptr == 0 {
            return Ok(Self::infinite());
        }
        let tv = copy_from_user_struct::<UserTimeVal>(ptr)?;
        if tv.sec < 0 || tv.usec < 0 || tv.usec >= 1_000_000 {
            return Err(ErrNo::EINVAL);
        }
        let ts = UserTimespec {
            sec: tv.sec,
            nsec: tv.usec * 1000,
        };
        Self::from_timespec(ts)
    }

    pub(crate) fn expired(&self) -> bool {
        match self.expire_tick {
            Some(exp) => task::current_tick() >= exp,
            None => false,
        }
    }

    pub(crate) fn remaining_ticks(&self) -> u64 {
        match self.expire_tick {
            Some(exp) => {
                let now = task::current_tick();
                if now >= exp {
                    0
                } else {
                    exp - now
                }
            }
            None => u64::MAX,
        }
    }
}

struct ScanCtx {
    fds_ptr: usize,
    nfds: usize,
}

impl ScanCtx {
    fn scan_count(&self) -> Result<usize, ErrNo> {
        let (n, _) = scan_pollfds(self.fds_ptr, self.nfds)?;
        Ok(n)
    }
}

pub(crate) fn poll_socket_revents(fd: usize, events: i16) -> i16 {
    let Some(socket) = socket_fd::lookup(fd) else {
        return 0;
    };
    let handle = socket.handle();
    let mut revents = 0i16;
    let kind = stack::socket_kind(handle);
    let state = stack::socket_state(handle);

    match kind {
        Ok(stack::SocketKind::Tcp) => match state {
            Ok(stack::SocketState::Listening { .. }) => {
                if events & POLLIN != 0
                    && stack::socket_has_pending_accept(handle).unwrap_or(false)
                {
                    revents |= POLLIN;
                }
            }
            Ok(stack::SocketState::Connecting) | Ok(stack::SocketState::Connected) => {
                if events & POLLIN != 0 && stack::socket_may_recv(handle).unwrap_or(false) {
                    revents |= POLLIN;
                }
                if events & POLLOUT != 0 && stack::socket_may_send(handle).unwrap_or(false) {
                    revents |= POLLOUT;
                }
                if stack::socket_is_connected(handle).unwrap_or(true) == false {
                    revents |= POLLHUP;
                }
            }
            Ok(stack::SocketState::Closed) => revents |= POLLHUP,
            _ => {}
        },
        Ok(stack::SocketKind::Udp) => {
            if events & POLLOUT != 0 {
                revents |= POLLOUT;
            }
            if events & POLLIN != 0 && stack::socket_udp_can_recv(handle).unwrap_or(false) {
                revents |= POLLIN;
            }
        }
        _ => {}
    }
    revents
}

pub(crate) fn poll_revents_fd(fd: usize, events: i16) -> i16 {
    if socket_fd::lookup(fd).is_some() {
        return poll_socket_revents(fd, events);
    }
    match vfs::fd::with_current_io(fd, |handle| handle.poll_revents(events)) {
        Ok(r) => r,
        Err(_) => POLLNVAL,
    }
}

pub(crate) fn scan_pollfds(fds_ptr: usize, nfds: usize) -> Result<(usize, usize), ErrNo> {
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

    for i in 0..nfds {
        let ptr = fds_ptr + i * pollfd_size;
        let mut pfd: PollFd = copy_from_user_struct(ptr)?;
        pfd.revents = 0;

        if pfd.fd < 0 {
            continue;
        }

        let fd = pfd.fd as usize;
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

fn poll_wait_pipe_fds(
    fds_ptr: usize,
    nfds: usize,
    deadline: &PollDeadline,
    still_waiting: &mut dyn FnMut() -> bool,
) -> Result<bool, ErrNo> {
    let pollfd_size = core::mem::size_of::<PollFd>();
    let remaining = deadline.remaining_ticks();
    if remaining == 0 {
        return Ok(false);
    }
    let mut any_pipe = false;
    for i in 0..nfds {
        let ptr = fds_ptr + i * pollfd_size;
        let pfd: PollFd = copy_from_user_struct(ptr)?;
        if pfd.fd < 0 {
            continue;
        }
        let fd = pfd.fd as usize;
        if socket_fd::lookup(fd).is_some() {
            continue;
        }
        match vfs::fd::with_current_io(fd, |handle| {
            handle.poll_wait_for_ticks(pfd.events, remaining, still_waiting)
        }) {
            Ok(()) => any_pipe = true,
            Err(vfs::api::VfsError::Interrupted) => return Err(ErrNo::EINTR),
            Err(_) => {}
        }
    }
    Ok(any_pipe)
}

pub(crate) fn poll_block_until_ready(
    fds_ptr: usize,
    nfds: usize,
    deadline: PollDeadline,
) -> Result<usize, ErrNo> {
    let ctx = ScanCtx { fds_ptr, nfds };
    loop {
        let n = ctx.scan_count()?;
        if n > 0 {
            return Ok(n);
        }
        if deadline.expired() {
            return Ok(0);
        }
        let remaining = deadline.remaining_ticks();
        if remaining == 0 {
            return Ok(0);
        }

        let mut still_waiting = || -> bool {
            ctx.scan_count().unwrap_or(0) == 0 && !deadline.expired()
        };
        let any_pipe = poll_wait_pipe_fds(fds_ptr, nfds, &deadline, &mut still_waiting)?;
        if !any_pipe {
            if task::sleep_for_ticks(1.min(remaining) as TaskTick) ==
               task::TaskWaitResult::Interrupted
            {
                return Err(ErrNo::EINTR);
            }
        }
    }
}

pub(crate) fn do_poll_with_deadline(
    fds_ptr: usize,
    nfds: usize,
    deadline: PollDeadline,
) -> UserRet {
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

pub(crate) fn validate_sigmask(sigmask_ptr: usize, sigsetsize: usize) -> Result<(), ErrNo> {
    if sigmask_ptr == 0 {
        return Ok(());
    }
    if sigsetsize != 8 {
        return Err(ErrNo::EINVAL);
    }
    Ok(())
}

pub(crate) fn fd_set_get(set: &FdSet, fd: usize) -> bool {
    if fd >= FD_SETSIZE {
        return false;
    }
    let word = fd / 64;
    let bit = fd % 64;
    (set[word] >> bit) & 1 != 0
}

pub(crate) fn fd_set_set(set: &mut FdSet, fd: usize) {
    if fd >= FD_SETSIZE {
        return;
    }
    let word = fd / 64;
    let bit = fd % 64;
    set[word] |= 1u64 << bit;
}

pub(crate) fn fd_set_clear(set: &mut FdSet, fd: usize) {
    if fd >= FD_SETSIZE {
        return;
    }
    let word = fd / 64;
    let bit = fd % 64;
    set[word] &= !(1u64 << bit);
}

pub(crate) fn copy_fd_set_from_user(ptr: usize) -> Result<Option<FdSet>, ErrNo> {
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

pub(crate) fn copy_fd_set_to_user(ptr: usize, set: &FdSet) -> Result<(), ErrNo> {
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

pub(crate) fn scan_fd_sets(
    nfds: usize,
    readfds_ptr: usize,
    writefds_ptr: usize,
    exceptfds_ptr: usize,
) -> Result<usize, ErrNo> {
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

    for fd in 0..nfds {
        let mut events = 0i16;
        if read_in.as_ref().is_some_and(|s| fd_set_get(s, fd)) {
            events |= POLLIN;
        }
        if write_in.as_ref().is_some_and(|s| fd_set_get(s, fd)) {
            events |= POLLOUT;
        }
        if except_in.as_ref().is_some_and(|s| fd_set_get(s, fd)) {
            events |= POLLPRI;
        }
        if events == 0 {
            continue;
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

    if read_in.is_some() {
        copy_fd_set_to_user(readfds_ptr, &read_out)?;
    }
    if write_in.is_some() {
        copy_fd_set_to_user(writefds_ptr, &write_out)?;
    }
    if except_in.is_some() {
        copy_fd_set_to_user(exceptfds_ptr, &except_out)?;
    }

    Ok(ready_count)
}

fn fd_monitored_in_sets(
    fd: usize,
    readfds_ptr: usize,
    writefds_ptr: usize,
    exceptfds_ptr: usize,
) -> Result<i16, ErrNo> {
    let read_in = copy_fd_set_from_user(readfds_ptr)?;
    let write_in = copy_fd_set_from_user(writefds_ptr)?;
    let except_in = copy_fd_set_from_user(exceptfds_ptr)?;
    let mut events = 0i16;
    if read_in.as_ref().is_some_and(|s| fd_set_get(s, fd)) {
        events |= POLLIN;
    }
    if write_in.as_ref().is_some_and(|s| fd_set_get(s, fd)) {
        events |= POLLOUT;
    }
    if except_in.as_ref().is_some_and(|s| fd_set_get(s, fd)) {
        events |= POLLPRI;
    }
    Ok(events)
}

fn poll_wait_monitored_fds(
    nfds: usize,
    readfds_ptr: usize,
    writefds_ptr: usize,
    exceptfds_ptr: usize,
    deadline: &PollDeadline,
    still_waiting: &mut dyn FnMut() -> bool,
) -> Result<bool, ErrNo> {
    let remaining = deadline.remaining_ticks();
    if remaining == 0 {
        return Ok(false);
    }
    let mut any_pipe = false;
    for fd in 0..nfds {
        let events = fd_monitored_in_sets(fd, readfds_ptr, writefds_ptr, exceptfds_ptr)?;
        if events == 0 {
            continue;
        }
        if socket_fd::lookup(fd).is_some() {
            continue;
        }
        match vfs::fd::with_current_io(fd, |handle| {
            handle.poll_wait_for_ticks(events, remaining, still_waiting)
        }) {
            Ok(()) => any_pipe = true,
            Err(vfs::api::VfsError::Interrupted) => return Err(ErrNo::EINTR),
            Err(_) => {}
        }
    }
    Ok(any_pipe)
}

pub(crate) fn poll_block_fd_sets(
    nfds: usize,
    readfds_ptr: usize,
    writefds_ptr: usize,
    exceptfds_ptr: usize,
    deadline: PollDeadline,
) -> Result<usize, ErrNo> {
    loop {
        let n = scan_fd_sets(nfds, readfds_ptr, writefds_ptr, exceptfds_ptr)?;
        if n > 0 {
            return Ok(n);
        }
        if deadline.expired() {
            return Ok(0);
        }
        let remaining = deadline.remaining_ticks();
        if remaining == 0 {
            return Ok(0);
        }
        let mut still_waiting = || -> bool {
            scan_fd_sets(nfds, readfds_ptr, writefds_ptr, exceptfds_ptr)
                .map(|c| c == 0)
                .unwrap_or(true)
                && !deadline.expired()
        };
        let any_pipe = poll_wait_monitored_fds(
            nfds,
            readfds_ptr,
            writefds_ptr,
            exceptfds_ptr,
            &deadline,
            &mut still_waiting,
        )?;
        if !any_pipe {
            if task::sleep_for_ticks(1.min(remaining) as TaskTick) ==
               task::TaskWaitResult::Interrupted
            {
                return Err(ErrNo::EINTR);
            }
        }
    }
}

pub(crate) fn do_pselect_with_deadline(
    nfds: usize,
    readfds_ptr: usize,
    writefds_ptr: usize,
    exceptfds_ptr: usize,
    deadline: PollDeadline,
) -> UserRet {
    let n = match scan_fd_sets(nfds, readfds_ptr, writefds_ptr, exceptfds_ptr) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };
    if n > 0 || deadline.expired() {
        return UserRet::from_success(n);
    }
    if deadline.remaining_ticks() == 0 {
        return UserRet::from_success(0);
    }
    match poll_block_fd_sets(nfds, readfds_ptr, writefds_ptr, exceptfds_ptr, deadline) {
        Ok(v) => UserRet::from_success(v),
        Err(e) => UserRet::from_error(e),
    }
}
