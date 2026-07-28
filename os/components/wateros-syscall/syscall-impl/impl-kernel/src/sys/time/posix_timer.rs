//! Process-scoped POSIX timer syscalls.

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::signal::{IntervalTimerSpec, PosixTimerClock, SignalError};

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const CLOCK_REALTIME : usize = 0;
const CLOCK_MONOTONIC : usize = 1;
const TIMER_ABSTIME : usize = 1;
const SIGEV_SIGNAL : i32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserItimerSpec {
    interval : UserTimespec,
    value : UserTimespec,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserSigevent {
    value : usize,
    signo : i32,
    notify : i32,
    reserved : [u8; 48],
}

const _ : () = assert!(core::mem::size_of::<UserSigevent>() == 64);
const _ : () = assert!(core::mem::size_of::<UserItimerSpec>() == 32);

pub(crate) fn sys_timer_create(args : SyscallArgs) -> UserRet {
    let clock = match args.arg(0) {
        CLOCK_REALTIME => PosixTimerClock::Realtime,
        CLOCK_MONOTONIC => PosixTimerClock::Monotonic,
        _ => return UserRet::from_error(ErrNo::EINVAL),
    };
    let event_ptr = args.arg(1);
    let timer_id_ptr = args.arg(2);
    if timer_id_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let signal = if event_ptr == 0 {
        ipc::signal::SIGALRM
    } else {
        let event = match copy_from_user_struct::<UserSigevent>(event_ptr) {
            Ok(event) => event,
            Err(error) => return UserRet::from_error(error),
        };
        if event.notify != SIGEV_SIGNAL ||
           event.signo <= 0 ||
           event.signo as usize > ipc::signal::NSIG
        {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        event.signo as usize
    };
    let pid = match task::current_process_snapshot() {
        Some(process) => process.pid.raw(),
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    if crate::sys::ipc::signal::ensure_current_signal_state().is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    let timer_id = match ipc::signal::create_posix_timer(pid, clock, signal) {
        Ok(timer_id) => timer_id,
        Err(error) => return UserRet::from_error(timer_error_to_errno(error)),
    };
    if let Err(error) = copy_to_user_struct(timer_id_ptr, &(timer_id as i32)) {
        let _ = ipc::signal::delete_posix_timer(pid, timer_id);
        return UserRet::from_error(error);
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_timer_settime(args : SyscallArgs) -> UserRet {
    let timer_id = match timer_id(args.arg(0)) {
        Ok(timer_id) => timer_id,
        Err(error) => return UserRet::from_error(error),
    };
    let flags = args.arg(1);
    let new_value_ptr = args.arg(2);
    let old_value_ptr = args.arg(3);
    if flags & !TIMER_ABSTIME != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if new_value_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let requested =
        match copy_from_user_struct::<UserItimerSpec>(new_value_ptr).and_then(user_spec_to_internal)
        {
            Ok(spec) => spec,
            Err(error) => return UserRet::from_error(error),
        };
    let (pid, monotonic_ns, realtime_ns) = match timer_context() {
        Ok(context) => context,
        Err(error) => return UserRet::from_error(error),
    };
    let old = match ipc::signal::set_posix_timer(pid,
                                                 timer_id,
                                                 requested,
                                                 monotonic_ns,
                                                 realtime_ns,
                                                 flags & TIMER_ABSTIME != 0)
    {
        Ok(old) => old,
        Err(error) => return UserRet::from_error(timer_error_to_errno(error)),
    };
    if old_value_ptr != 0 {
        if let Err(error) = copy_to_user_struct(old_value_ptr,
                                                &internal_spec_to_user(old))
        {
            return UserRet::from_error(error);
        }
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_timer_gettime(args : SyscallArgs) -> UserRet {
    let timer_id = match timer_id(args.arg(0)) {
        Ok(timer_id) => timer_id,
        Err(error) => return UserRet::from_error(error),
    };
    let value_ptr = args.arg(1);
    if value_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let (pid, monotonic_ns, realtime_ns) = match timer_context() {
        Ok(context) => context,
        Err(error) => return UserRet::from_error(error),
    };
    let spec = match ipc::signal::get_posix_timer(pid, timer_id, monotonic_ns, realtime_ns) {
        Ok(spec) => spec,
        Err(error) => return UserRet::from_error(timer_error_to_errno(error)),
    };
    match copy_to_user_struct(value_ptr, &internal_spec_to_user(spec)) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

pub(crate) fn sys_timer_getoverrun(args : SyscallArgs) -> UserRet {
    let timer_id = match timer_id(args.arg(0)) {
        Ok(timer_id) => timer_id,
        Err(error) => return UserRet::from_error(error),
    };
    let pid = match task::current_process_snapshot() {
        Some(process) => process.pid.raw(),
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    match ipc::signal::get_posix_timer_overrun(pid, timer_id) {
        Ok(overrun) => UserRet::from_success(overrun as usize),
        Err(error) => UserRet::from_error(timer_error_to_errno(error)),
    }
}

pub(crate) fn sys_timer_delete(args : SyscallArgs) -> UserRet {
    let timer_id = match timer_id(args.arg(0)) {
        Ok(timer_id) => timer_id,
        Err(error) => return UserRet::from_error(error),
    };
    let pid = match task::current_process_snapshot() {
        Some(process) => process.pid.raw(),
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    match ipc::signal::delete_posix_timer(pid, timer_id) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(timer_error_to_errno(error)),
    }
}

fn timer_context() -> Result<(usize, u128, u128), ErrNo> {
    let pid = task::current_process_snapshot().map(|process| process.pid.raw())
                                              .ok_or(ErrNo::ESRCH)?;
    let monotonic_ns = platform::wall_clock::monotonic_ns().map_err(|_| ErrNo::EIO)?;
    let realtime_ns = platform::wall_clock::realtime_ns().map_err(|_| ErrNo::EIO)?;
    Ok((pid, monotonic_ns, realtime_ns))
}

fn timer_id(raw : usize) -> Result<usize, ErrNo> {
    if raw > i32::MAX as usize {
        Err(ErrNo::EINVAL)
    } else {
        Ok(raw)
    }
}

fn user_spec_to_internal(spec : UserItimerSpec) -> Result<IntervalTimerSpec, ErrNo> {
    Ok(IntervalTimerSpec { interval_ns : timespec_to_ns(spec.interval)?,
                           value_ns : timespec_to_ns(spec.value)? })
}

fn timespec_to_ns(value : UserTimespec) -> Result<u128, ErrNo> {
    if value.sec < 0 || value.nsec < 0 || value.nsec >= 1_000_000_000 {
        return Err(ErrNo::EINVAL);
    }
    (value.sec as u128).checked_mul(1_000_000_000)
                       .and_then(|seconds| seconds.checked_add(value.nsec as u128))
                       .ok_or(ErrNo::EINVAL)
}

fn internal_spec_to_user(spec : IntervalTimerSpec) -> UserItimerSpec {
    UserItimerSpec { interval : ns_to_timespec(spec.interval_ns),
                     value : ns_to_timespec(spec.value_ns) }
}

fn ns_to_timespec(ns : u128) -> UserTimespec {
    UserTimespec { sec : isize::try_from(ns / 1_000_000_000).unwrap_or(isize::MAX),
                   nsec : (ns % 1_000_000_000) as isize }
}

fn timer_error_to_errno(error : SignalError) -> ErrNo {
    match error {
        SignalError::NoSuchProcess | SignalError::NoSuchTask => ErrNo::ESRCH,
        _ => ErrNo::EINVAL,
    }
}
