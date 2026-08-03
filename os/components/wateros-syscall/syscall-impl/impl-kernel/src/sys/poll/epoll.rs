//! `epoll_create1` / `epoll_ctl` / `epoll_wait` / `epoll_pwait`。

//! 本模块代码由AI完成
extern crate alloc;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use task::TaskTick;

use crate::epoll_fd::{
    self, epoll_to_poll_events, poll_to_epoll_events, EpollEvent, EpollHandle, EpollInterest,
    EPOLL_CLOEXEC, EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD, EPOLLERR, EPOLLHUP,
    EPOLL_VALID_EVENTS,
};
use crate::poll_engine::{
    poll_revents_fd, PollDeadline, POLLIN, POLLNVAL, POLLOUT, POLLPRI,
};
use crate::user_copy::{copy_from_user, copy_to_user};
use crate::vfs_util::vfs_error_to_errno;

const FD_CLOEXEC: usize = 1;

// 本方法代码由AI完成
pub(crate) fn sys_epoll_create1(args: SyscallArgs) -> UserRet {
    let flags = args.arg(0);
    if flags & !EPOLL_CLOEXEC != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    create_epoll_fd(flags)
}

fn create_epoll_fd(flags: usize) -> UserRet {
    let task_id = match vfs::fd::current_task_id() {
        Ok(task_id) => task_id,
        Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
    };
    let (handle, instance) = EpollHandle::new_pair();
    let fd = match vfs::fd::with_registry(|reg| {
        reg.alloc_fd_for_task(task_id, alloc::boxed::Box::new(handle))
    }) {
        Ok(fd) => fd,
        Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
    };
    epoll_fd::register(fd, instance);
    if flags & EPOLL_CLOEXEC != 0 {
        if let Err(err) = vfs::fd::set_fd_flags(fd, FD_CLOEXEC) {
            let _ = vfs::fd::close_fd(fd);
            epoll_fd::remove(fd);
            return UserRet::from_error(vfs_error_to_errno(err));
        }
    }
    UserRet::from_success(fd)
}

// 本方法代码由AI完成
pub(crate) fn sys_epoll_ctl(args: SyscallArgs) -> UserRet {
    let epfd = args.arg(0);
    let op = args.arg(1);
    let target_fd = args.arg(2);
    let event_ptr = args.arg(3);

    let Some(instance) = epoll_fd::lookup(epfd) else {
        if vfs::fd::with_current_io(epfd, |_| Ok(())).is_ok() {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        return UserRet::from_error(ErrNo::EBADF);
    };

    if target_fd == epfd {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    match op {
        EPOLL_CTL_ADD | EPOLL_CTL_MOD => {
            if event_ptr == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let event = match read_epoll_event(event_ptr) {
                Ok(event) => event,
                Err(err) => return UserRet::from_error(err),
            };
            if event.events == 0 || event.events & !EPOLL_VALID_EVENTS != 0 {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            if !fd_is_pollable(target_fd) {
                return UserRet::from_error(ErrNo::EBADF);
            }
            let mut guard = instance.lock();
            match op {
                EPOLL_CTL_ADD => {
                    if guard.interests.contains_key(&target_fd) {
                        return UserRet::from_error(ErrNo::EEXIST);
                    }
                    guard.interests.insert(
                        target_fd,
                        EpollInterest {
                            events: event.events,
                            data: event.data,
                        },
                    );
                }
                EPOLL_CTL_MOD => {
                    if !guard.interests.contains_key(&target_fd) {
                        return UserRet::from_error(ErrNo::ENOENT);
                    }
                    guard.interests.insert(
                        target_fd,
                        EpollInterest {
                            events: event.events,
                            data: event.data,
                        },
                    );
                }
                _ => {}
            }
            UserRet::from_success(0)
        }
        EPOLL_CTL_DEL => {
            let mut guard = instance.lock();
            if !guard.interests.remove(&target_fd).is_some() {
                return UserRet::from_error(ErrNo::ENOENT);
            }
            UserRet::from_success(0)
        }
        _ => UserRet::from_error(ErrNo::EINVAL),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_epoll_wait(args: SyscallArgs) -> UserRet {
    let epfd = args.arg(0);
    let events_ptr = args.arg(1);
    let maxevents = args.arg(2);
    let timeout_ms = args.arg(3) as isize;
    do_epoll_wait(epfd, events_ptr, maxevents, timeout_ms)
}

// 本方法代码由AI完成
pub(crate) fn sys_epoll_pwait(args: SyscallArgs) -> UserRet {
    let epfd = args.arg(0);
    let events_ptr = args.arg(1);
    let maxevents = args.arg(2);
    let timeout_ms = args.arg(3) as isize;
    // sigmask 暂未实现，委托 epoll_wait。
    let _ = (args.arg(4), args.arg(5));
    do_epoll_wait(epfd, events_ptr, maxevents, timeout_ms)
}

fn do_epoll_wait(
    epfd: usize,
    events_ptr: usize,
    maxevents: usize,
    timeout_ms: isize,
) -> UserRet {
    if maxevents <= 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if events_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let Some(instance) = epoll_fd::lookup(epfd) else {
        if vfs::fd::with_current_io(epfd, |_| Ok(())).is_ok() {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        return UserRet::from_error(ErrNo::EBADF);
    };

    let deadline = match PollDeadline::from_poll_millis(timeout_ms) {
        Ok(d) => d,
        Err(e) => return UserRet::from_error(e),
    };

    loop {
        if instance.lock().is_closed() {
            return UserRet::from_error(ErrNo::EBADF);
        }
        match scan_epoll_ready(&instance, events_ptr, maxevents) {
            Ok(0) => {}
            Ok(n) => return UserRet::from_success(n),
            Err(e) => return UserRet::from_error(e),
        }

        if deadline.expired() {
            return UserRet::from_success(0);
        }
        if deadline.remaining_ticks() == 0 {
            return UserRet::from_success(0);
        }

        let mut still_waiting = || -> bool {
            scan_epoll_ready(&instance, events_ptr, maxevents)
                .map(|n| n == 0 && !deadline.expired())
                .unwrap_or(false)
        };
        match epoll_wait_interests(&instance, &deadline, &mut still_waiting) {
            Ok(true) => continue,
            Ok(false) => {
                if task::sleep_for_ticks(1.min(deadline.remaining_ticks()) as TaskTick)
                    == task::TaskWaitResult::Interrupted
                {
                    return UserRet::from_error(ErrNo::EINTR);
                }
            }
            Err(e) => return UserRet::from_error(e),
        }
    }
}

fn fd_is_pollable(fd: usize) -> bool {
    poll_revents_fd(fd, POLLIN | POLLOUT | POLLPRI) != POLLNVAL
}

fn read_epoll_event(ptr : usize) -> Result<EpollEvent, ErrNo> {
    let mut bytes = [0; EpollEvent::ABI_SIZE];
    let copied = copy_from_user(&mut bytes, ptr)?;
    if copied != bytes.len() {
        return Err(ErrNo::EFAULT);
    }
    Ok(EpollEvent::from_abi_bytes(&bytes))
}

fn scan_epoll_ready(
    instance: &alloc::sync::Arc<spin::Mutex<epoll_fd::EpollInstance>>,
    events_ptr: usize,
    maxevents: usize,
) -> Result<usize, ErrNo> {
    crate::poll_engine::drive_network_stack();
    let interests : alloc::vec::Vec<(usize, EpollInterest)> = {
        let guard = instance.lock();
        guard.interests
             .iter()
             .map(|(&fd, interest)| (fd, interest.clone()))
             .collect()
    };
    let mut ready = 0usize;
    let event_size = EpollEvent::ABI_SIZE;

    for (fd, interest) in interests {
        if ready >= maxevents {
            break;
        }
        let poll_events = epoll_to_poll_events(interest.events);
        let revents = poll_revents_fd(fd, poll_events);
        if revents & POLLNVAL != 0 {
            continue;
        }
        let mut out_events = poll_to_epoll_events(revents);
        out_events &= interest.events | EPOLLERR | EPOLLHUP;
        if out_events == 0 {
            continue;
        }
        let out = EpollEvent {
            events: out_events,
            data: interest.data,
        };
        let ptr = events_ptr + ready * event_size;
        let bytes = out.to_abi_bytes();
        if copy_to_user(ptr, &bytes)? != bytes.len() {
            return Err(ErrNo::EFAULT);
        }
        ready += 1;
    }
    Ok(ready)
}

fn epoll_wait_interests(
    instance: &alloc::sync::Arc<spin::Mutex<epoll_fd::EpollInstance>>,
    deadline: &PollDeadline,
    still_waiting: &mut dyn FnMut() -> bool,
) -> Result<bool, ErrNo> {
    let remaining = deadline.remaining_ticks();
    if remaining == 0 {
        return Ok(false);
    }
    let wait_ticks = remaining.min(1);
    let interests: alloc::vec::Vec<(usize, i16)> = {
        let guard = instance.lock();
        guard
            .interests
            .iter()
            .map(|(&fd, interest)| (fd, epoll_to_poll_events(interest.events)))
            .collect()
    };

    let mut any_wait = false;
    for (fd, events) in interests {
        if !still_waiting() {
            return Ok(true);
        }
        if crate::socket_fd::lookup(fd).is_some() {
            continue;
        }
        let mut wait_on_this_fd = || !deadline.expired();
        match vfs::fd::with_current_io(fd, |handle| {
            handle.poll_wait_for_ticks(events, wait_ticks, &mut wait_on_this_fd)
        }) {
            Ok(()) => any_wait = true,
            Err(vfs::api::VfsError::Interrupted) => return Err(ErrNo::EINTR),
            Err(_) => {}
        }
    }
    Ok(any_wait)
}
