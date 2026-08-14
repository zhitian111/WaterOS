//! Linux `timerfd_create(2)`、`timerfd_settime(2)` 与 `timerfd_gettime(2)`。
//!
//! timerfd 是普通 VFS fd 对象；`dup` 与 `fork` 共享同一状态。超时次数在读取、
//! poll 或查询时按单调/实时时钟结算，不需要额外的全局 timer worker。

extern crate alloc;

use alloc::{boxed::Box, sync::Arc};

use api_v0::{ErrNo, SyscallArgs, UserRet};
use spin::Mutex;
use vfs::{
    api::{
        VfsCopyProgress, VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsPreparedRead,
        VfsReadFinish, VfsReadLease, VfsResult,
    },
    fd,
};

use crate::{
    poll_engine::ns_duration_to_ticks,
    user_copy::{copy_from_user_struct, copy_to_user_struct},
    vfs_util::vfs_error_to_errno,
};

const CLOCK_REALTIME : usize = 0;
const CLOCK_MONOTONIC : usize = 1;

const TFD_TIMER_ABSTIME : usize = 1;
const TFD_TIMER_CANCEL_ON_SET : usize = 2;
const TFD_NONBLOCK : usize = 0o0004000;
const TFD_CLOEXEC : usize = 0o2000000;
const FD_CLOEXEC : usize = 1;

const POLLIN : i16 = 0x001;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct UserItimerSpec {
    interval : UserTimespec,
    value : UserTimespec,
}

const _ : () = assert!(core::mem::size_of::<UserItimerSpec>() == 32);

#[derive(Clone, Copy)]
enum TimerFdClock {
    Realtime,
    Monotonic,
}

impl TimerFdClock {
    fn now_ns(self) -> VfsResult<u128> {
        match self {
            Self::Realtime => platform::wall_clock::realtime_ns(),
            Self::Monotonic => platform::wall_clock::monotonic_ns(),
        }
        .map_err(|_| VfsError::Io)
    }
}

#[derive(Clone, Copy)]
struct TimerReadReservation {
    id : u64,
    generation : u64,
    expirations : u64,
}

struct TimerFdInner {
    next_expiration_ns : Option<u128>,
    interval_ns : u128,
    pending_expirations : u64,
    nonblocking : bool,
    generation : u64,
    next_read_id : u64,
    read_reservation : Option<TimerReadReservation>,
}

impl TimerFdInner {
    fn new(nonblocking : bool) -> Self {
        Self { next_expiration_ns : None,
               interval_ns : 0,
               pending_expirations : 0,
               nonblocking,
               generation : 1,
               next_read_id : 1,
               read_reservation : None }
    }

    /// 将已经到期的周期折叠为一个累计计数，并推进下一次期限。
    fn refresh(&mut self, now_ns : u128) {
        let Some(deadline) = self.next_expiration_ns else {
            return;
        };
        if now_ns < deadline {
            return;
        }
        let expirations = if self.interval_ns == 0 {
            1
        } else {
            1u128.saturating_add((now_ns - deadline) / self.interval_ns)
        };
        self.pending_expirations = self.pending_expirations
                                       .saturating_add(u64::try_from(expirations)
                                                               .unwrap_or(u64::MAX));
        self.next_expiration_ns = if self.interval_ns == 0 {
            None
        } else {
            Some(deadline.saturating_add(expirations.saturating_mul(self.interval_ns)))
        };
    }

    fn remaining_spec(&self, now_ns : u128) -> UserItimerSpec {
        UserItimerSpec { interval : ns_to_timespec(self.interval_ns),
                         value : ns_to_timespec(self.next_expiration_ns
                                                    .map(|deadline| {
                                                        deadline.saturating_sub(now_ns)
                                                    })
                                                    .unwrap_or(0)) }
    }
}

struct TimerFdState {
    clock : TimerFdClock,
    inner : Mutex<TimerFdInner>,
    wait : task::wait_queue::WaitQueue,
}

impl TimerFdState {
    fn new(clock : TimerFdClock, nonblocking : bool) -> Self {
        Self { clock,
               inner : Mutex::new(TimerFdInner::new(nonblocking)),
               wait : task::wait_queue::WaitQueue::new_named("timerfd") }
    }

    fn reserve_read(&self) -> VfsResult<TimerReadReservation> {
        loop {
            let (nonblocking, wait_ticks) = {
                let now = self.clock.now_ns()?;
                let mut inner = self.inner.lock();
                inner.refresh(now);
                if inner.read_reservation.is_none() && inner.pending_expirations != 0 {
                    let reservation = TimerReadReservation {
                        id : inner.next_read_id,
                        generation : inner.generation,
                        expirations : inner.pending_expirations,
                    };
                    inner.next_read_id = inner.next_read_id.wrapping_add(1);
                    inner.pending_expirations = 0;
                    inner.read_reservation = Some(reservation);
                    return Ok(reservation);
                }
                let ticks = inner.next_expiration_ns
                                 .map(|deadline| {
                                     ns_duration_to_ticks(deadline.saturating_sub(now)).max(1)
                                 });
                (inner.nonblocking, ticks)
            };
            if nonblocking {
                return Err(VfsError::WouldBlock);
            }

            let wait_result = if let Some(ticks) = wait_ticks {
                self.wait.wait_current_while_for_ticks(ticks, || self.should_wait())
            } else {
                self.wait.wait_current_while(|| self.should_wait())
            };
            if wait_result == task::TaskWaitResult::Interrupted {
                return Err(VfsError::Interrupted);
            }
        }
    }

    fn should_wait(&self) -> bool {
        let Ok(now) = self.clock.now_ns() else {
            return false;
        };
        let mut inner = self.inner.lock();
        inner.refresh(now);
        inner.read_reservation.is_some() || inner.pending_expirations == 0
    }

    fn finish_read(&self,
                   reservation : TimerReadReservation,
                   commit : bool)
                   -> VfsResult<()> {
        let mut inner = self.inner.lock();
        let active = inner.read_reservation.ok_or(VfsError::Io)?;
        if active.id != reservation.id {
            return Err(VfsError::Io);
        }
        if !commit && active.generation == inner.generation {
            inner.pending_expirations = inner.pending_expirations
                                       .saturating_add(active.expirations);
        }
        inner.read_reservation = None;
        drop(inner);
        self.wait.wake_all();
        Ok(())
    }

    fn cancel_read(&self, reservation : TimerReadReservation) {
        let mut inner = self.inner.lock();
        if inner.read_reservation.is_some_and(|active| active.id == reservation.id) {
            if reservation.generation == inner.generation {
                inner.pending_expirations = inner.pending_expirations
                                           .saturating_add(reservation.expirations);
            }
            inner.read_reservation = None;
            drop(inner);
            self.wait.wake_all();
        }
    }

    fn ready(&self, events : i16) -> VfsResult<i16> {
        let now = self.clock.now_ns()?;
        let mut inner = self.inner.lock();
        inner.refresh(now);
        Ok(if events & POLLIN != 0 &&
              inner.pending_expirations != 0 &&
              inner.read_reservation.is_none()
           {
               POLLIN
           } else {
               0
           })
    }

    fn settime(&self, requested : UserItimerSpec, absolute : bool) -> VfsResult<UserItimerSpec> {
        let now = self.clock.now_ns()?;
        let interval_ns = timespec_to_ns(requested.interval).map_err(|_| VfsError::InvalidPath)?;
        let value_ns = timespec_to_ns(requested.value).map_err(|_| VfsError::InvalidPath)?;
        let old = {
            let mut inner = self.inner.lock();
            inner.refresh(now);
            let old = inner.remaining_spec(now);
            inner.generation = inner.generation.wrapping_add(1);
            inner.pending_expirations = 0;
            inner.interval_ns = interval_ns;
            inner.next_expiration_ns = if value_ns == 0 {
                None
            } else if absolute {
                Some(value_ns)
            } else {
                Some(now.saturating_add(value_ns))
            };
            inner.refresh(now);
            old
        };
        self.wait.wake_all();
        Ok(old)
    }

    fn gettime(&self) -> VfsResult<UserItimerSpec> {
        let now = self.clock.now_ns()?;
        let mut inner = self.inner.lock();
        inner.refresh(now);
        Ok(inner.remaining_spec(now))
    }
}

impl Drop for TimerFdState {
    fn drop(&mut self) {
        self.wait.wake_all();
        let _ = self.wait.try_release_empty();
    }
}

struct TimerFdHandle {
    state : Arc<TimerFdState>,
}

impl TimerFdHandle {
    fn new(clock : TimerFdClock, nonblocking : bool) -> Self {
        Self { state : Arc::new(TimerFdState::new(clock, nonblocking)) }
    }
}

impl VfsIoHandle for TimerFdHandle {
    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        if max_len < core::mem::size_of::<u64>() {
            return Err(VfsError::InvalidPath);
        }
        Ok(Box::new(TimerFdPreparedRead { state : self.state.clone() }))
    }

    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> {
        let prepared = self.prepare_read(buf.len())?;
        let lease = prepared.acquire()?;
        buf[..8].copy_from_slice(lease.bytes());
        match lease.finish(VfsCopyProgress { copied : 8, complete : true })? {
            VfsReadFinish::Bytes(8) => Ok(8),
            _ => Err(VfsError::Io),
        }
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(VfsMetadata { node_type : VfsNodeType::Special,
                         size : 0,
                         mode : 0o600,
                         device_major : 0,
                         device_minor : 0,
                         inode : Arc::as_ptr(&self.state) as usize as u64,
                         mount_id : 0,
                         nlink : 1,
                         uid : 0,
                         gid : 0 })
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self { state : self.state.clone() }))
    }

    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> { self.state.ready(events) }

    fn poll_wait_for_ticks(&mut self,
                           events : i16,
                           timeout_ticks : u64,
                           still_waiting : &mut dyn FnMut() -> bool)
                           -> VfsResult<()> {
        if self.state.ready(events)? != 0 || timeout_ticks == 0 {
            return Ok(());
        }
        let result = self.state.wait.wait_current_while_for_ticks(timeout_ticks, || {
            still_waiting() && self.state.ready(events).unwrap_or(0) == 0
        });
        if result == task::TaskWaitResult::Interrupted {
            Err(VfsError::Interrupted)
        } else {
            Ok(())
        }
    }

    fn open_status_flags(&self) -> u32 {
        if self.state.inner.lock().nonblocking { TFD_NONBLOCK as u32 } else { 0 }
    }

    fn open_accmode(&self) -> u32 { 0 }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        self.state.inner.lock().nonblocking = flags & TFD_NONBLOCK as u32 != 0;
        Ok(())
    }
}

struct TimerFdPreparedRead {
    state : Arc<TimerFdState>,
}

impl VfsPreparedRead for TimerFdPreparedRead {
    fn acquire(self : Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        let reservation = self.state.reserve_read()?;
        Ok(Box::new(TimerFdReadLease { state : self.state,
                                      reservation : Some(reservation),
                                      bytes : reservation.expirations.to_ne_bytes() }))
    }
}

struct TimerFdReadLease {
    state : Arc<TimerFdState>,
    reservation : Option<TimerReadReservation>,
    bytes : [u8; 8],
}

impl VfsReadLease for TimerFdReadLease {
    fn bytes(&self) -> &[u8] { &self.bytes }

    fn finish(mut self : Box<Self>, progress : VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied > self.bytes.len() {
            return Err(VfsError::Io);
        }
        let reservation = self.reservation.take().ok_or(VfsError::Io)?;
        let complete = progress.complete && progress.copied == self.bytes.len();
        self.state.finish_read(reservation, complete)?;
        if complete {
            Ok(VfsReadFinish::Bytes(self.bytes.len()))
        } else {
            Ok(VfsReadFinish::Fault)
        }
    }
}

impl Drop for TimerFdReadLease {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            self.state.cancel_read(reservation);
        }
    }
}

pub(crate) fn sys_timerfd_create(args : SyscallArgs) -> UserRet {
    let clock = match args.arg(0) {
        CLOCK_REALTIME => TimerFdClock::Realtime,
        CLOCK_MONOTONIC => TimerFdClock::Monotonic,
        _ => return UserRet::from_error(ErrNo::EINVAL),
    };
    let flags = args.arg(1);
    if flags & !(TFD_NONBLOCK | TFD_CLOEXEC) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let timer_fd = match fd::alloc_fd(Box::new(TimerFdHandle::new(clock,
                                                                  flags & TFD_NONBLOCK != 0))) {
        Ok(fd) => fd,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    if flags & TFD_CLOEXEC != 0 {
        if let Err(error) = fd::set_fd_flags(timer_fd, FD_CLOEXEC) {
            let _ = fd::close_fd(timer_fd);
            return UserRet::from_error(vfs_error_to_errno(error));
        }
    }
    UserRet::from_success(timer_fd)
}

pub(crate) fn sys_timerfd_settime(args : SyscallArgs) -> UserRet {
    let timer_fd = args.arg(0);
    let flags = args.arg(1);
    let new_value_ptr = args.arg(2);
    let old_value_ptr = args.arg(3);
    if flags & !(TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET) != 0 ||
       flags & TFD_TIMER_CANCEL_ON_SET != 0
    {
        // CANCEL_ON_SET 需要把 wall-clock 跳变以 ECANCELED 传递到 read；当前错误面
        // 尚不能表达该事件，因此明确拒绝，不能静默当作普通 timer。
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if new_value_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let requested = match copy_from_user_struct::<UserItimerSpec>(new_value_ptr) {
        Ok(spec) => spec,
        Err(error) => return UserRet::from_error(error),
    };
    if timespec_to_ns(requested.interval).is_err() || timespec_to_ns(requested.value).is_err() {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let result = vfs::fd::with_current_io(timer_fd, |handle| {
        let timer = handle.as_any().downcast_ref::<TimerFdHandle>()
                          .ok_or(VfsError::InvalidPath)?;
        timer.state.settime(requested, flags & TFD_TIMER_ABSTIME != 0)
    });
    let old = match result {
        Ok(old) => old,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    if old_value_ptr != 0 {
        if let Err(error) = copy_to_user_struct(old_value_ptr, &old) {
            return UserRet::from_error(error);
        }
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_timerfd_gettime(args : SyscallArgs) -> UserRet {
    let timer_fd = args.arg(0);
    let value_ptr = args.arg(1);
    if value_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let value = match vfs::fd::with_current_io(timer_fd, |handle| {
        let timer = handle.as_any().downcast_ref::<TimerFdHandle>()
                          .ok_or(VfsError::InvalidPath)?;
        timer.state.gettime()
    }) {
        Ok(value) => value,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    match copy_to_user_struct(value_ptr, &value) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

fn timespec_to_ns(value : UserTimespec) -> Result<u128, ErrNo> {
    if value.sec < 0 || value.nsec < 0 || value.nsec >= 1_000_000_000 {
        return Err(ErrNo::EINVAL);
    }
    Ok((value.sec as u128).saturating_mul(1_000_000_000)
                              .saturating_add(value.nsec as u128))
}

fn ns_to_timespec(ns : u128) -> UserTimespec {
    UserTimespec { sec : isize::try_from(ns / 1_000_000_000).unwrap_or(isize::MAX),
                   nsec : (ns % 1_000_000_000) as isize }
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    let mut periodic = TimerFdInner::new(false);
    periodic.next_expiration_ns = Some(100);
    periodic.interval_ns = 20;
    periodic.refresh(149);
    assert_eq!(periodic.pending_expirations, 3);
    assert_eq!(periodic.next_expiration_ns, Some(160));

    let mut one_shot = TimerFdInner::new(false);
    one_shot.next_expiration_ns = Some(100);
    one_shot.refresh(100);
    assert_eq!(one_shot.pending_expirations, 1);
    assert_eq!(one_shot.next_expiration_ns, None);

    assert_eq!(timespec_to_ns(UserTimespec { sec : 1, nsec : 2 }),
               Ok(1_000_000_002));
    assert_eq!(timespec_to_ns(UserTimespec { sec : -1, nsec : 0 }),
               Err(ErrNo::EINVAL));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periodic_expirations_are_accumulated_and_deadline_is_advanced() {
        let mut inner = TimerFdInner::new(false);
        inner.next_expiration_ns = Some(100);
        inner.interval_ns = 20;
        inner.refresh(149);
        assert_eq!(inner.pending_expirations, 3);
        assert_eq!(inner.next_expiration_ns, Some(160));
    }

    #[test]
    fn one_shot_disarms_after_expiration() {
        let mut inner = TimerFdInner::new(false);
        inner.next_expiration_ns = Some(100);
        inner.refresh(100);
        assert_eq!(inner.pending_expirations, 1);
        assert_eq!(inner.next_expiration_ns, None);
    }

    #[test]
    fn timespec_validation_matches_linux_bounds() {
        assert_eq!(timespec_to_ns(UserTimespec { sec : 1, nsec : 2 }), Ok(1_000_000_002));
        assert_eq!(timespec_to_ns(UserTimespec { sec : -1, nsec : 0 }), Err(ErrNo::EINVAL));
        assert_eq!(timespec_to_ns(UserTimespec { sec : 0, nsec : 1_000_000_000 }),
                   Err(ErrNo::EINVAL));
    }
}
