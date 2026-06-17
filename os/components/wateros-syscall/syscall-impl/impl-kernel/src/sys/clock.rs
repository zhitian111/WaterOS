//! 时钟类系统调用：`clock_gettime` / `clock_settime` / `clock_getres` /
//! `clock_nanosleep`，以及 `gettimeofday` / `nanosleep` 的统一时间语义。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use platform::timer;
use platform::wall_clock::{realtime_ns, set_realtime_ns};
use wateros_base_config::task::SCHED_TIMER_PERIOD_MS;

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const CLOCK_REALTIME: usize = 0;
const CLOCK_MONOTONIC: usize = 1;
const CLOCK_PROCESS_CPUTIME_ID: usize = 2;
const CLOCK_MONOTONIC_RAW: usize = 4;
const CLOCK_REALTIME_COARSE: usize = 5;
const CLOCK_MONOTONIC_COARSE: usize = 6;

const TIMER_ABSTIME: usize = 1;

const SCHED_TICK_NS: u128 = (SCHED_TIMER_PERIOD_MS as u128) * 1_000_000;
const HIGH_RES_CLOCK_NS: u128 = 1;
const HIGH_RES_FALLBACK_NS: u128 = 1_000;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimeVal {
    sec : isize,
    usec : isize,
}

fn monotonic_now_ns() -> Result<u128, ErrNo> {
    match timer::now_duration() {
        Ok(duration) => Ok(duration.as_nanos()),
        Err(_) => {
            let tick = task::current_tick().max(1);
            Ok((tick as u128) * SCHED_TICK_NS)
        }
    }
}

fn timespec_resolution_ns() -> u128 {
    match timer::tick_hz() {
        Ok(hz) if hz.0 > 0 => 1_000_000_000u128 / (hz.0 as u128),
        _ => HIGH_RES_FALLBACK_NS,
    }
}

fn timespec_to_ns(ts : UserTimespec) -> Result<u128, ErrNo> {
    if ts.sec < 0 || ts.nsec < 0 || ts.nsec >= 1_000_000_000 {
        return Err(ErrNo::EINVAL);
    }
    Ok((ts.sec as u128) * 1_000_000_000 + ts.nsec as u128)
}

fn ns_to_timespec(ns : u128) -> UserTimespec {
    UserTimespec { sec: (ns / 1_000_000_000) as isize, nsec: (ns % 1_000_000_000) as isize }
}

fn is_supported_getres_clock(clock_id : usize) -> bool {
    matches!(clock_id,
             CLOCK_REALTIME |
             CLOCK_MONOTONIC |
             CLOCK_PROCESS_CPUTIME_ID |
             CLOCK_MONOTONIC_RAW |
             CLOCK_REALTIME_COARSE |
             CLOCK_MONOTONIC_COARSE)
}

fn is_sleepable_clock(clock_id : usize) -> bool {
    matches!(clock_id,
             CLOCK_REALTIME |
             CLOCK_MONOTONIC |
             CLOCK_MONOTONIC_RAW |
             CLOCK_REALTIME_COARSE |
             CLOCK_MONOTONIC_COARSE)
}

fn clock_id_to_ns(clock_id : usize) -> Result<u128, ErrNo> {
    match clock_id {
        CLOCK_REALTIME | CLOCK_REALTIME_COARSE => {
            realtime_ns().map_err(|_| ErrNo::EIO)
        }
        CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW | CLOCK_MONOTONIC_COARSE => monotonic_now_ns(),
        CLOCK_PROCESS_CPUTIME_ID => {
            let snapshot = task::current_task_snapshot()
                                 .ok_or(ErrNo::ESRCH)?;
            Ok((snapshot.stats.tick_count as u128) * SCHED_TICK_NS)
        }
        _ => Err(ErrNo::EINVAL),
    }
}

fn clock_id_to_timespec(clock_id : usize) -> Result<UserTimespec, ErrNo> {
    Ok(ns_to_timespec(clock_id_to_ns(clock_id)?))
}

fn sleep_for_ns(total_ns : u128, rem_ptr : usize) -> UserRet {
    if total_ns == 0 {
        return UserRet::from_success(0);
    }
    let start = match monotonic_now_ns() {
        Ok(now) => now,
        Err(error) => return UserRet::from_error(error),
    };
    let ticks = ((total_ns + SCHED_TICK_NS - 1) / SCHED_TICK_NS).max(1);
    let ticks = u64::try_from(ticks).unwrap_or(u64::MAX);
    if task::sleep_for_ticks(ticks) != task::TaskWaitResult::Interrupted {
        return UserRet::from_success(0);
    }
    if rem_ptr != 0 {
        let elapsed = monotonic_now_ns()
            .unwrap_or(start)
            .saturating_sub(start);
        let remaining = total_ns.saturating_sub(elapsed);
        if let Err(error) = copy_to_user_struct(rem_ptr, &ns_to_timespec(remaining)) {
            return UserRet::from_error(error);
        }
    }
    UserRet::from_error(ErrNo::EINTR)
}

fn write_zero_timespec(ptr : usize) -> Result<(), ErrNo> {
    if ptr == 0 {
        return Ok(());
    }
    copy_to_user_struct(ptr, &UserTimespec { sec: 0, nsec: 0 })
}

pub(crate) fn sys_gettimeofday(args : SyscallArgs) -> UserRet {
    let timeval_ptr = args.arg(0);
    if timeval_ptr == 0 {
        return UserRet::from_success(0);
    }
    let ns = match clock_id_to_ns(CLOCK_REALTIME) {
        Ok(ns) => ns,
        Err(e) => return UserRet::from_error(e),
    };
    let timeval = UserTimeVal {
        sec: (ns / 1_000_000_000) as isize,
        usec: ((ns % 1_000_000_000) / 1_000) as isize,
    };
    match copy_to_user_struct(timeval_ptr, &timeval) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

pub(crate) fn sys_clock_gettime(args : SyscallArgs) -> UserRet {
    let clock_id = args.arg(0);
    let timespec_ptr = args.arg(1);
    if timespec_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let timespec = match clock_id_to_timespec(clock_id) {
        Ok(ts) => ts,
        Err(e) => return UserRet::from_error(e),
    };
    match copy_to_user_struct(timespec_ptr, &timespec) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

pub(crate) fn sys_clock_settime(args : SyscallArgs) -> UserRet {
    let clock_id = args.arg(0);
    let tp_ptr = args.arg(1);
    if clock_id != CLOCK_REALTIME {
        return UserRet::from_error(ErrNo::EPERM);
    }
    if tp_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let target = match copy_from_user_struct::<UserTimespec>(tp_ptr) {
        Ok(ts) => ts,
        Err(e) => return UserRet::from_error(e),
    };
    let target_ns = match timespec_to_ns(target) {
        Ok(ns) => ns,
        Err(e) => return UserRet::from_error(e),
    };
    if set_realtime_ns(target_ns).is_err() {
        return UserRet::from_error(ErrNo::EIO);
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_clock_getres(args : SyscallArgs) -> UserRet {
    let clock_id = args.arg(0);
    let res_ptr = args.arg(1);
    if !is_supported_getres_clock(clock_id) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if res_ptr == 0 {
        return UserRet::from_success(0);
    }
    let res_ns = match clock_id {
        CLOCK_REALTIME_COARSE | CLOCK_MONOTONIC_COARSE | CLOCK_PROCESS_CPUTIME_ID => SCHED_TICK_NS,
        CLOCK_REALTIME | CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW => HIGH_RES_CLOCK_NS,
        _ => timespec_resolution_ns(),
    };
    let res = ns_to_timespec(res_ns);
    match copy_to_user_struct(res_ptr, &res) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

pub(crate) fn sys_clock_nanosleep(args : SyscallArgs) -> UserRet {
    let clock_id = args.arg(0);
    let flags = args.arg(1);
    let req_ptr = args.arg(2);
    let rem_ptr = args.arg(3);
    if !is_sleepable_clock(clock_id) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & !TIMER_ABSTIME != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if req_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let req = match copy_from_user_struct::<UserTimespec>(req_ptr) {
        Ok(req) => req,
        Err(e) => return UserRet::from_error(e),
    };
    let rel = if flags & TIMER_ABSTIME != 0 {
        let target_ns = match timespec_to_ns(req) {
            Ok(ns) => ns,
            Err(e) => return UserRet::from_error(e),
        };
        let now_ns = match clock_id_to_ns(clock_id) {
            Ok(ns) => ns,
            Err(e) => return UserRet::from_error(e),
        };
        if target_ns <= now_ns {
            if let Err(e) = write_zero_timespec(rem_ptr) {
                return UserRet::from_error(e);
            }
            return UserRet::from_success(0);
        }
        ns_to_timespec(target_ns - now_ns)
    } else {
        req
    };
    if rel.sec == 0 && rel.nsec == 0 {
        if let Err(e) = write_zero_timespec(rem_ptr) {
            return UserRet::from_error(e);
        }
        return UserRet::from_success(0);
    }
    let ret = sleep_for_ns(match timespec_to_ns(rel) {
        Ok(ns) => ns,
        Err(e) => return UserRet::from_error(e),
    }, if flags & TIMER_ABSTIME == 0 { rem_ptr } else { 0 });
    if ret.0 >= 0 && rem_ptr != 0 {
        if let Err(e) = write_zero_timespec(rem_ptr) {
            return UserRet::from_error(e);
        }
    }
    ret
}

pub(crate) fn sys_nanosleep(args : SyscallArgs) -> UserRet {
    let req_ptr = args.arg(0);
    let rem_ptr = args.arg(1);
    if req_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let req = match copy_from_user_struct::<UserTimespec>(req_ptr) {
        Ok(req) => req,
        Err(e) => return UserRet::from_error(e),
    };
    if req.sec == 0 && req.nsec == 0 {
        return UserRet::from_success(0);
    }
    let total_ns = match timespec_to_ns(req) {
        Ok(ns) => ns,
        Err(e) => return UserRet::from_error(e),
    };
    sleep_for_ns(total_ns, rem_ptr)
}
