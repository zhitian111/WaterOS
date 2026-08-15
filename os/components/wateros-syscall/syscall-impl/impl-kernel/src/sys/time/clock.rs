//! 时钟类系统调用：`clock_gettime` / `clock_settime` / `clock_getres` /
//! `clock_nanosleep`，以及 `gettimeofday` / `nanosleep` 的统一时间语义。

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use platform::timer;
use platform::wall_clock::{realtime_ns, set_realtime_ns};
use spin::Mutex;
use wateros_base_config::task::SCHED_TIMER_PERIOD_MS;

use crate::poll_engine::ns_duration_to_ticks;
use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const CLOCK_REALTIME : usize = 0;
const CLOCK_MONOTONIC : usize = 1;
const CLOCK_PROCESS_CPUTIME_ID : usize = 2;
const CLOCK_THREAD_CPUTIME_ID : usize = 3;
const CLOCK_MONOTONIC_RAW : usize = 4;
const CLOCK_REALTIME_COARSE : usize = 5;
const CLOCK_MONOTONIC_COARSE : usize = 6;
const CLOCK_BOOTTIME : usize = 7;

const TIMER_ABSTIME : usize = 1;

const SCHED_TICK_NS : u128 = (SCHED_TIMER_PERIOD_MS as u128) * 1_000_000;
const SCHED_TICK_US : i64 = (SCHED_TIMER_PERIOD_MS as i64) * 1_000;
const ADJ_OFFSET : u32 = 0x0001;
const ADJ_FREQUENCY : u32 = 0x0002;
const ADJ_MAXERROR : u32 = 0x0004;
const ADJ_ESTERROR : u32 = 0x0008;
const ADJ_STATUS : u32 = 0x0010;
const ADJ_TIMECONST : u32 = 0x0020;
const ADJ_TAI : u32 = 0x0080;
const ADJ_SETOFFSET : u32 = 0x0100;
const ADJ_MICRO : u32 = 0x1000;
const ADJ_NANO : u32 = 0x2000;
const ADJ_TICK : u32 = 0x4000;
const ADJ_OFFSET_SINGLESHOT : u32 = 0x8001;
const ADJ_OFFSET_SS_READ : u32 = 0xA001;
const ADJ_REGULAR_MASK : u32 = ADJ_OFFSET |
                               ADJ_FREQUENCY |
                               ADJ_MAXERROR |
                               ADJ_ESTERROR |
                               ADJ_STATUS |
                               ADJ_TIMECONST |
                               ADJ_TAI |
                               ADJ_SETOFFSET |
                               ADJ_MICRO |
                               ADJ_NANO |
                               ADJ_TICK;
const STA_NANO : i32 = 0x2000;
const TIME_OK : usize = 0;

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

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimex {
    modes : u32,
    offset : i64,
    freq : i64,
    maxerror : i64,
    esterror : i64,
    status : i32,
    constant : i64,
    precision : i64,
    tolerance : i64,
    time : UserTimeVal,
    tick : i64,
    ppsfreq : i64,
    jitter : i64,
    shift : i32,
    stabil : i64,
    jitcnt : i64,
    calcnt : i64,
    errcnt : i64,
    stbcnt : i64,
    tai : i32,
    _reserved : [i32; 11],
}

#[derive(Clone, Copy)]
struct TimexState {
    offset : i64,
    freq : i64,
    maxerror : i64,
    esterror : i64,
    status : i32,
    constant : i64,
    tick : i64,
    tai : i32,
}

static TIMEX_STATE : Mutex<TimexState> = Mutex::new(TimexState { offset : 0,
                                                                 freq : 0,
                                                                 maxerror : 0,
                                                                 esterror : 0,
                                                                 status : STA_NANO,
                                                                 constant : 0,
                                                                 tick : SCHED_TICK_US,
                                                                 tai : 0 });

fn monotonic_now_ns() -> Result<u128, ErrNo> {
    match timer::now_duration() {
        Ok(duration) => Ok(duration.as_nanos()),
        Err(_) => {
            let tick = task::current_tick().max(1);
            Ok((tick as u128) * SCHED_TICK_NS)
        }
    }
}

fn timespec_to_ns(ts : UserTimespec) -> Result<u128, ErrNo> {
    if ts.sec < 0 || ts.nsec < 0 || ts.nsec >= 1_000_000_000 {
        return Err(ErrNo::EINVAL);
    }
    Ok((ts.sec as u128) * 1_000_000_000 + ts.nsec as u128)
}

fn ns_to_timespec(ns : u128) -> UserTimespec {
    UserTimespec { sec : (ns / 1_000_000_000) as isize,
                   nsec : (ns % 1_000_000_000) as isize }
}

fn is_supported_getres_clock(clock_id : usize) -> bool {
    matches!(clock_id,
             CLOCK_REALTIME |
             CLOCK_MONOTONIC |
             CLOCK_PROCESS_CPUTIME_ID |
             CLOCK_THREAD_CPUTIME_ID |
             CLOCK_MONOTONIC_RAW |
             CLOCK_REALTIME_COARSE |
             CLOCK_MONOTONIC_COARSE |
             CLOCK_BOOTTIME)
}

fn is_sleepable_clock(clock_id : usize) -> bool {
    matches!(clock_id,
             CLOCK_REALTIME |
             CLOCK_MONOTONIC |
             CLOCK_MONOTONIC_RAW |
             CLOCK_REALTIME_COARSE |
             CLOCK_MONOTONIC_COARSE |
             CLOCK_BOOTTIME)
}

fn clock_id_to_ns(clock_id : usize) -> Result<u128, ErrNo> {
    match clock_id {
        CLOCK_REALTIME | CLOCK_REALTIME_COARSE => realtime_ns().map_err(|_| ErrNo::EIO),
        CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW | CLOCK_MONOTONIC_COARSE | CLOCK_BOOTTIME => {
            monotonic_now_ns()
        }
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            let snapshot = task::current_task_snapshot().ok_or(ErrNo::ESRCH)?;
            Ok((snapshot.stats
                        .tick_count as u128) *
               SCHED_TICK_NS)
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
    let deadline = start.saturating_add(total_ns);
    loop {
        let now = monotonic_now_ns().unwrap_or(start);
        if now >= deadline {
            return UserRet::from_success(0);
        }
        let ticks = ns_duration_to_ticks(deadline - now);
        if task::sleep_for_ticks(ticks) == task::TaskWaitResult::Interrupted {
            if rem_ptr != 0 {
                let remaining = deadline.saturating_sub(monotonic_now_ns().unwrap_or(now));
                if let Err(error) = copy_to_user_struct(rem_ptr, &ns_to_timespec(remaining)) {
                    return UserRet::from_error(error);
                }
            }
            return UserRet::from_error(ErrNo::EINTR);
        }
    }
}

fn sleep_until_ns(clock_id : usize, target_ns : u128) -> UserRet {
    loop {
        let now_ns = match clock_id_to_ns(clock_id) {
            Ok(now) => now,
            Err(error) => return UserRet::from_error(error),
        };
        if target_ns <= now_ns {
            return UserRet::from_success(0);
        }
        let ret = sleep_for_ns(target_ns - now_ns, 0);
        if ret.0 < 0 {
            return ret;
        }
    }
}

fn write_zero_timespec(ptr : usize) -> Result<(), ErrNo> {
    if ptr == 0 {
        return Ok(());
    }
    copy_to_user_struct(ptr, &UserTimespec { sec : 0, nsec : 0 })
}

fn valid_adjtimex_modes(modes : u32) -> bool {
    modes == ADJ_OFFSET_SINGLESHOT ||
    modes == ADJ_OFFSET_SS_READ ||
    (modes & !ADJ_REGULAR_MASK) == 0
}

fn timex_snapshot(state : TimexState) -> UserTimex {
    let ns = clock_id_to_ns(CLOCK_REALTIME).unwrap_or(0);
    UserTimex { modes : 0,
                offset : state.offset,
                freq : state.freq,
                maxerror : state.maxerror,
                esterror : state.esterror,
                status : state.status,
                constant : state.constant,
                precision : SCHED_TICK_US,
                tolerance : 32_768_000,
                time : UserTimeVal { sec : (ns / 1_000_000_000) as isize,
                                     usec : ((ns % 1_000_000_000) / 1_000) as isize },
                tick : state.tick,
                ppsfreq : 0,
                jitter : 0,
                shift : 0,
                stabil : 0,
                jitcnt : 0,
                calcnt : 0,
                errcnt : 0,
                stbcnt : 0,
                tai : state.tai,
                _reserved : [0; 11] }
}

fn update_timex_state(state : &mut TimexState, timex : UserTimex) -> Result<(), ErrNo> {
    let modes = timex.modes;
    if modes & ADJ_TICK != 0 && !(9_000..=11_000).contains(&timex.tick) {
        return Err(ErrNo::EINVAL);
    }
    if modes & ADJ_OFFSET != 0 || modes == ADJ_OFFSET_SINGLESHOT {
        state.offset = timex.offset;
    }
    if modes & ADJ_FREQUENCY != 0 {
        state.freq = timex.freq;
    }
    if modes & ADJ_MAXERROR != 0 {
        state.maxerror = timex.maxerror;
    }
    if modes & ADJ_ESTERROR != 0 {
        state.esterror = timex.esterror;
    }
    if modes & ADJ_STATUS != 0 {
        state.status = timex.status;
    }
    if modes & ADJ_TIMECONST != 0 {
        state.constant = timex.constant;
    }
    if modes & ADJ_TAI != 0 {
        state.tai = timex.tai;
    }
    if modes & ADJ_TICK != 0 {
        state.tick = timex.tick;
    }
    if modes & ADJ_NANO != 0 {
        state.status |= STA_NANO;
    }
    if modes & ADJ_MICRO != 0 {
        state.status &= !STA_NANO;
    }
    Ok(())
}

fn do_adjtimex(clock_id : usize, timex_ptr : usize) -> UserRet {
    if clock_id != CLOCK_REALTIME {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let timex = match copy_from_user_struct::<UserTimex>(timex_ptr) {
        Ok(timex) => timex,
        Err(e) => return UserRet::from_error(e),
    };
    if !valid_adjtimex_modes(timex.modes) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let write_only_mode = timex.modes != 0 && timex.modes != ADJ_OFFSET_SS_READ;
    if write_only_mode &&
       cred::current_credentials().effective_uid
                                  .0 !=
       0
    {
        return UserRet::from_error(ErrNo::EPERM);
    }

    let snapshot = {
        let mut state = TIMEX_STATE.lock();
        if write_only_mode {
            if let Err(e) = update_timex_state(&mut state, timex) {
                return UserRet::from_error(e);
            }
        }
        timex_snapshot(*state)
    };
    match copy_to_user_struct(timex_ptr, &snapshot) {
        Ok(()) => UserRet::from_success(TIME_OK),
        Err(e) => UserRet::from_error(e),
    }
}

pub(crate) fn sys_adjtimex(args : SyscallArgs) -> UserRet {
    do_adjtimex(CLOCK_REALTIME, args.arg(0))
}

pub(crate) fn sys_clock_adjtime(args : SyscallArgs) -> UserRet {
    do_adjtimex(args.arg(0), args.arg(1))
}

pub(crate) fn sys_gettimeofday(args : SyscallArgs) -> UserRet {
    let timeval_ptr = args.arg(0);
    let timezone_ptr = args.arg(1);
    if timeval_ptr != 0 {
        let ns = match clock_id_to_ns(CLOCK_REALTIME) {
            Ok(ns) => ns,
            Err(e) => return UserRet::from_error(e),
        };
        let timeval = UserTimeVal { sec : (ns / 1_000_000_000) as isize,
                                    usec : ((ns % 1_000_000_000) / 1_000) as isize };
        if let Err(error) = copy_to_user_struct(timeval_ptr, &timeval) {
            return UserRet::from_error(error);
        }
    }
    if timezone_ptr != 0 {
        // `struct timezone` 已废弃，但 Linux 的原始 syscall 仍会校验并
        // 写入非空指针。忽略该参数会把坏地址错误地报告成成功。
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct UserTimezone {
            minutes_west : i32,
            dst_time : i32,
        }
        let timezone = UserTimezone { minutes_west : 0,
                                      dst_time : 0 };
        if let Err(error) = copy_to_user_struct(timezone_ptr, &timezone) {
            return UserRet::from_error(error);
        }
    }
    UserRet::from_success(0)
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
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if cred::current_credentials().effective_uid
                                  .0 !=
       0
    {
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

/// `settimeofday(2)`：设置实时时钟；需要 effective 持有 CAP_SYS_TIME。
/// timezone 参数（arg1）已废弃，忽略。
pub(crate) fn sys_settimeofday(args : SyscallArgs) -> UserRet {
    let tv_ptr = args.arg(0);
    if tv_ptr == 0 {
        // Linux：tv == NULL 时成功（仅清 offset）。
        return UserRet::from_success(0);
    }
    // Linux：权限看 effective 是否持有 CAP_SYS_TIME（与 euid 无关）。
    // WaterOS root 默认 effective 含 CAP_SYS_TIME；LTP settimeofday02 用
    // TST_CAP_DROP 移除后应返回 EPERM。
    let caps = task::current_process_task_snapshot().map(|snapshot| snapshot.pid)
                                                    .and_then(|pid| task::process_caps(pid))
                                                    .unwrap_or(task::ProcessCaps::ROOT);
    if caps.effective & task::ProcessCaps::CAP_SYS_TIME == 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }
    let tv = match copy_from_user_struct::<UserTimeVal>(tv_ptr) {
        Ok(tv) => tv,
        Err(e) => return UserRet::from_error(e),
    };
    if tv.sec < 0 || tv.usec < 0 || tv.usec >= 1_000_000 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let target_ns = (tv.sec as u128) * 1_000_000_000 + (tv.usec as u128) * 1_000;
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
    let res_ns = SCHED_TICK_NS;
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
    if matches!(clock_id,
                CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID)
    {
        return UserRet::from_error(ErrNo::EOPNOTSUPP);
    }
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
    if flags & TIMER_ABSTIME != 0 {
        let target_ns = match timespec_to_ns(req) {
            Ok(ns) => ns,
            Err(e) => return UserRet::from_error(e),
        };
        let ret = sleep_until_ns(clock_id, target_ns);
        if ret.0 >= 0 && rem_ptr != 0 {
            if let Err(e) = write_zero_timespec(rem_ptr) {
                return UserRet::from_error(e);
            }
        }
        return ret;
    }
    if req.sec == 0 && req.nsec == 0 {
        if let Err(e) = write_zero_timespec(rem_ptr) {
            return UserRet::from_error(e);
        }
        return UserRet::from_success(0);
    }
    let ret = sleep_for_ns(match timespec_to_ns(req) {
                               Ok(ns) => ns,
                               Err(e) => return UserRet::from_error(e),
                           },
                           rem_ptr);
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
