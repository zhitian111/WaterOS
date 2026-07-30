//! `eventfd2(2)` counter descriptors.

extern crate alloc;

use alloc::{boxed::Box, sync::Arc};

use api_v0::{ErrNo, SyscallArgs, UserRet};
use spin::Mutex;
use vfs::{
    api::{VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsResult},
    fd,
};

use crate::vfs_util::vfs_error_to_errno;

const EFD_SEMAPHORE : usize = 1;
const EFD_NONBLOCK : usize = 0o0004000;
const EFD_CLOEXEC : usize = 0o2000000;
const FD_CLOEXEC : usize = 1;
const POLLIN : i16 = 0x001;
const POLLOUT : i16 = 0x004;
const MAX_COUNTER : u64 = u64::MAX - 1;

struct EventFdInner {
    counter : u64,
    nonblocking : bool,
}

struct EventFdState {
    inner : Mutex<EventFdInner>,
    wait : task::wait_queue::WaitQueue,
}

impl EventFdState {
    fn new(counter : u64, nonblocking : bool) -> Self {
        Self { inner : Mutex::new(EventFdInner { counter,
                                                 nonblocking }),
               wait : task::wait_queue::WaitQueue::new_named("eventfd") }
    }

    fn ready(&self, events : i16) -> i16 {
        let inner = self.inner.lock();
        let mut ready = 0;
        if events & POLLIN != 0 && inner.counter != 0 {
            ready |= POLLIN;
        }
        if events & POLLOUT != 0 && inner.counter < MAX_COUNTER {
            ready |= POLLOUT;
        }
        ready
    }
}

impl Drop for EventFdState {
    fn drop(&mut self) {
        self.wait.wake_all();
        let _ = self.wait
                    .try_release_empty();
    }
}

struct EventFdHandle {
    state : Arc<EventFdState>,
    semaphore : bool,
}

impl EventFdHandle {
    fn new(counter : u64, nonblocking : bool, semaphore : bool) -> Self {
        Self { state : Arc::new(EventFdState::new(counter, nonblocking)),
               semaphore }
    }
}

impl VfsIoHandle for EventFdHandle {
    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> {
        if buf.len() < core::mem::size_of::<u64>() {
            return Err(VfsError::InvalidPath);
        }
        loop {
            {
                let mut inner = self.state
                                    .inner
                                    .lock();
                if inner.counter != 0 {
                    let value = if self.semaphore { 1 } else { inner.counter };
                    inner.counter -= value;
                    buf[..8].copy_from_slice(&value.to_ne_bytes());
                    drop(inner);
                    self.state
                        .wait
                        .wake_all();
                    return Ok(8);
                }
                if inner.nonblocking {
                    return Err(VfsError::WouldBlock);
                }
            }
            if self.state
                   .wait
                   .wait_current_while(|| {
                       self.state
                           .inner
                           .lock()
                           .counter ==
                       0
                   }) ==
               task::TaskWaitResult::Interrupted
            {
                return Err(VfsError::Interrupted);
            }
        }
    }

    fn write(&mut self, buf : &[u8]) -> VfsResult<usize> {
        if buf.len() < core::mem::size_of::<u64>() {
            return Err(VfsError::InvalidPath);
        }
        let value = u64::from_ne_bytes(buf[..8].try_into()
                                               .expect("eventfd write width"));
        if value == u64::MAX {
            return Err(VfsError::InvalidPath);
        }
        loop {
            {
                let mut inner = self.state
                                    .inner
                                    .lock();
                if inner.counter <= MAX_COUNTER - value {
                    inner.counter += value;
                    drop(inner);
                    self.state
                        .wait
                        .wake_all();
                    return Ok(8);
                }
                if inner.nonblocking {
                    return Err(VfsError::WouldBlock);
                }
            }
            if self.state
                   .wait
                   .wait_current_while(|| {
                       self.state
                           .inner
                           .lock()
                           .counter >
                       MAX_COUNTER - value
                   }) ==
               task::TaskWaitResult::Interrupted
            {
                return Err(VfsError::Interrupted);
            }
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
        Ok(Box::new(Self { state : self.state
                                       .clone(),
                           semaphore:
                               self.semaphore }))
    }

    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
        Ok(self.state
               .ready(events))
    }

    fn poll_wait_for_ticks(&mut self,
                           events : i16,
                           timeout_ticks : u64,
                           still_waiting : &mut dyn FnMut() -> bool)
                           -> VfsResult<()> {
        if self.state
               .ready(events) !=
           0 ||
           timeout_ticks == 0
        {
            return Ok(());
        }
        let result = self.state
                         .wait
                         .wait_current_while_for_ticks(timeout_ticks, || {
                             still_waiting() &&
                             self.state
                                 .ready(events) ==
                             0
                         });
        if result == task::TaskWaitResult::Interrupted {
            Err(VfsError::Interrupted)
        } else {
            Ok(())
        }
    }

    fn open_status_flags(&self) -> u32 {
        if self.state
               .inner
               .lock()
               .nonblocking
        {
            EFD_NONBLOCK as u32
        } else {
            0
        }
    }

    fn open_accmode(&self) -> u32 { 2 }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        self.state
            .inner
            .lock()
            .nonblocking = flags & EFD_NONBLOCK as u32 != 0;
        Ok(())
    }
}

pub(crate) fn sys_eventfd2(args : SyscallArgs) -> UserRet {
    let initial = args.arg(0) as u32 as u64;
    let flags = args.arg(1);
    if flags & !(EFD_SEMAPHORE | EFD_NONBLOCK | EFD_CLOEXEC) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let handle = EventFdHandle::new(initial,
                                    flags & EFD_NONBLOCK != 0,
                                    flags & EFD_SEMAPHORE != 0);
    let event_fd = match fd::alloc_fd(Box::new(handle)) {
        Ok(fd) => fd,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    if flags & EFD_CLOEXEC != 0 {
        if let Err(error) = fd::set_fd_flags(event_fd, FD_CLOEXEC) {
            let _ = fd::close_fd(event_fd);
            return UserRet::from_error(vfs_error_to_errno(error));
        }
    }
    UserRet::from_success(event_fd)
}
