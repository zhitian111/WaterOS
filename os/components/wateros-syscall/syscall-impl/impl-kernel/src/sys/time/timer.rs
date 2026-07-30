//! 时间与资源统计类系统调用：`times`、`getrusage`、`getitimer`、`setitimer`。
//! 也包含子进程 CPU 时间统计（`ChildCpuTicks`），供 wait 模块在收子进程时调用。

use alloc::collections::BTreeMap;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use ipc::signal::IntervalTimerSpec;
use spin::Mutex;

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const RUSAGE_CHILDREN : isize = -1;
const RUSAGE_SELF : isize = 0;
const RUSAGE_THREAD : isize = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTms {
    utime : isize,
    stime : isize,
    cutime : isize,
    cstime : isize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct UserTimeVal {
    pub(crate) sec : isize,
    pub(crate) usec : isize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserITimerVal {
    interval : UserTimeVal,
    value : UserTimeVal,
}

/// Linux LP64 `struct rusage` 布局。
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct UserRUsage {
    pub(crate) utime : UserTimeVal,
    pub(crate) stime : UserTimeVal,
    maxrss : isize,
    ixrss : isize,
    idrss : isize,
    isrss : isize,
    minflt : isize,
    majflt : isize,
    nswap : isize,
    inblock : isize,
    oublock : isize,
    msgsnd : isize,
    msgrcv : isize,
    nsignals : isize,
    nvcsw : isize,
    nivcsw : isize,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ChildCpuTicks {
    pub(crate) utime : isize,
    pub(crate) stime : isize,
}

static CHILD_CPU : Mutex<BTreeMap<usize, ChildCpuTicks>> = Mutex::new(BTreeMap::new());

const _ : () = assert!(core::mem::size_of::<UserRUsage>() == 144);
const _ : () = assert!(core::mem::size_of::<UserITimerVal>() == 32);

pub(crate) fn ticks_to_timeval(ticks : isize) -> UserTimeVal {
    UserTimeVal { sec : ticks / 100,
                  usec : (ticks % 100) * 10_000 }
}

pub(crate) fn write_zero_rusage(rusage_ptr : usize) -> Result<(), ErrNo> {
    if rusage_ptr == 0 {
        return Ok(());
    }
    copy_to_user_struct(rusage_ptr, &UserRUsage::default())
}

pub(crate) fn write_child_rusage(rusage_ptr : usize,
                                 child_cpu : ChildCpuTicks)
                                 -> Result<(), ErrNo> {
    if rusage_ptr == 0 {
        return Ok(());
    }
    let mut usage = UserRUsage::default();
    usage.utime = ticks_to_timeval(child_cpu.utime);
    usage.stime = ticks_to_timeval(child_cpu.stime);
    copy_to_user_struct(rusage_ptr, &usage)
}

pub(crate) fn child_cpu_from_exited(exited_tasks : &[task::ExitedTask]) -> ChildCpuTicks {
    let ticks = exited_tasks.iter()
                            .map(|exited| {
                                exited.stats
                                      .tick_count as isize
                            })
                            .sum();
    ChildCpuTicks { utime : ticks,
                    stime : ticks }
}

pub(crate) fn account_child_cpu(parent_pid : task::ProcessId, child_cpu : ChildCpuTicks) {
    let mut table = CHILD_CPU.lock();
    let entry = table.entry(parent_pid.raw())
                     .or_default();
    entry.utime = entry.utime
                       .saturating_add(child_cpu.utime);
    entry.stime = entry.stime
                       .saturating_add(child_cpu.stime);
}

pub(crate) fn child_cpu_ticks(pid : task::ProcessId) -> ChildCpuTicks {
    CHILD_CPU.lock()
             .get(&pid.raw())
             .copied()
             .unwrap_or_default()
}

pub(crate) fn sys_times(args : SyscallArgs) -> UserRet {
    let tms_ptr = args.arg(0);
    if tms_ptr != 0 {
        let snapshot = match task::current_task_snapshot() {
            Some(snapshot) => snapshot,
            None => return UserRet::from_error(ErrNo::ESRCH),
        };
        let child_cpu =
            task::current_process_task_snapshot().map(|process| child_cpu_ticks(process.pid))
                                                 .unwrap_or_default();
        let ticks = snapshot.stats
                            .tick_count as isize;
        let tms = UserTms { utime : snapshot.stats
                                            .tick_count
                                    as isize,
                            stime : ticks,
                            cutime : child_cpu.utime,
                            cstime : child_cpu.stime };
        if let Err(e) = copy_to_user_struct(tms_ptr, &tms) {
            return UserRet::from_error(e);
        }
    }
    UserRet::from_success(task::current_tick() as usize)
}

pub(crate) fn sys_getrusage(args : SyscallArgs) -> UserRet {
    let who = args.arg(0) as isize;
    let usage_ptr = args.arg(1);
    if usage_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let mut usage = UserRUsage::default();
    match who {
        RUSAGE_SELF | RUSAGE_THREAD => {
            let snapshot = match task::current_task_snapshot() {
                Some(snapshot) => snapshot,
                None => return UserRet::from_error(ErrNo::ESRCH),
            };
            let ticks = snapshot.stats
                                .tick_count as isize;
            usage.utime = ticks_to_timeval(ticks);
        }
        RUSAGE_CHILDREN => {
            if let Some(process) = task::current_process_task_snapshot() {
                let child_cpu = child_cpu_ticks(process.pid);
                usage.utime = ticks_to_timeval(child_cpu.utime);
                usage.stime = ticks_to_timeval(child_cpu.stime);
            }
        }
        _ => return UserRet::from_error(ErrNo::EINVAL),
    }
    match copy_to_user_struct(usage_ptr, &usage) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

fn valid_timeval(tv : UserTimeVal) -> bool { tv.sec >= 0 && tv.usec >= 0 && tv.usec < 1_000_000 }

fn timeval_to_ns(tv : UserTimeVal) -> u128 {
    (tv.sec as u128).saturating_mul(1_000_000_000)
                    .saturating_add((tv.usec as u128).saturating_mul(1_000))
}

fn timer_spec_to_user(spec : IntervalTimerSpec) -> UserITimerVal {
    fn ns_to_timeval_ceil(ns : u128) -> UserTimeVal {
        if ns == 0 {
            return UserTimeVal::default();
        }
        let usec = ns.saturating_add(999) / 1_000;
        UserTimeVal { sec : (usec / 1_000_000) as isize,
                      usec : (usec % 1_000_000) as isize }
    }
    UserITimerVal { interval : ns_to_timeval_ceil(spec.interval_ns),
                    value : ns_to_timeval_ceil(spec.value_ns) }
}

pub(crate) fn sys_setitimer(args : SyscallArgs) -> UserRet {
    let which = args.arg(0);
    let new_value = args.arg(1);
    let old_value = args.arg(2);

    if !ipc::signal::valid_itimer(which) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if new_value == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let value = match copy_from_user_struct::<UserITimerVal>(new_value) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(e),
    };
    if !valid_timeval(value.interval) || !valid_timeval(value.value) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let snapshot = match crate::sys::ipc::signal::ensure_current_signal_state() {
        Ok(snapshot) => snapshot,
        Err(error) => return UserRet::from_error(error),
    };
    let now = match platform::wall_clock::monotonic_ns() {
        Ok(now) => now,
        Err(_) => return UserRet::from_error(ErrNo::EIO),
    };
    let spec = IntervalTimerSpec { interval_ns : timeval_to_ns(value.interval),
                                   value_ns : timeval_to_ns(value.value) };
    if old_value != 0 {
        if let Err(error) = copy_to_user_struct(old_value, &UserITimerVal::default()) {
            return UserRet::from_error(error);
        }
    }
    let old = match ipc::signal::set_timer(snapshot.pid.raw(), which, spec, now) {
        Ok(old) => old,
        Err(_) => return UserRet::from_error(ErrNo::EINVAL),
    };
    if old_value != 0 {
        let old = timer_spec_to_user(old);
        if let Err(e) = copy_to_user_struct(old_value, &old) {
            return UserRet::from_error(e);
        }
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_getitimer(args : SyscallArgs) -> UserRet {
    let which = args.arg(0);
    let current_value = args.arg(1);
    if !ipc::signal::valid_itimer(which) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if current_value == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let snapshot = match crate::sys::ipc::signal::ensure_current_signal_state() {
        Ok(snapshot) => snapshot,
        Err(error) => return UserRet::from_error(error),
    };
    let now = match platform::wall_clock::monotonic_ns() {
        Ok(now) => now,
        Err(_) => return UserRet::from_error(ErrNo::EIO),
    };
    let spec = match ipc::signal::get_timer(snapshot.pid.raw(), which, now) {
        Ok(spec) => spec,
        Err(_) => return UserRet::from_error(ErrNo::EINVAL),
    };
    match copy_to_user_struct(current_value, &timer_spec_to_user(spec)) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}
