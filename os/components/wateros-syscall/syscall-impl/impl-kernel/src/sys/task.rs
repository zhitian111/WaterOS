//! 任务相关系统调用：`yield`、`exit`、`waitpid`、`getpid`/`getppid`/`gettid`、
//! `times`、`uname`、`prctl`、`getrlimit`/`setrlimit`。

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::signal::{IntervalTimerSpec, SignalAction, SignalError, SignalSet};
use task::{ResourceLimit, SetResourceLimitError};

use crate::user_copy::{copy_from_user_struct, copy_to_user, copy_to_user_struct};

const ORPHAN_PARENT_PID : usize = 1;
const WNOHANG : usize = 1;
const WUNTRACED : usize = 2;
const WCONTINUED : usize = 8;
const WAITPID_IGNORED_OPTIONS : usize = WUNTRACED | WCONTINUED;
const UTS_LEN : usize = 65;

// prctl 操作码
const PR_SET_NAME : usize = 15;
const PR_GET_NAME : usize = 16;
const PR_SET_NO_NEW_PRIVS : usize = 38;
const PR_CAPBSET_READ : usize = 23;
const PR_CAPBSET_DROP : usize = 24;

// rlimit 资源号
const RLIMIT_STACK : usize = 3;
const RLIMIT_NOFILE : usize = 7;
const RLIMIT_AS : usize = 9;
const RLIMIT_DATA : usize = 2;
const RLIMIT_CORE : usize = 4;
const RLIMIT_MEMLOCK : usize = 8;
const RLIMIT_NPROC : usize = 6;
const RT_SIGSET_SIZE_64 : usize = 8;
const RT_SIGACTION_SIZE : usize = 24;
const GRND_NONBLOCK : usize = 0x0001;
const GRND_RANDOM : usize = 0x0002;
const GRND_INSECURE : usize = 0x0004;
const GETRANDOM_ALLOWED_FLAGS : usize = GRND_NONBLOCK | GRND_RANDOM | GRND_INSECURE;
static CURRENT_UMASK : AtomicUsize = AtomicUsize::new(0o022);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserSigAction {
    handler : usize,
    flags : usize,
    mask : u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserSigInfo {
    signo : i32,
    errno : i32,
    code : i32,
    payload : [u8; 116],
}

impl UserSigInfo {
    fn for_signal(sig : usize) -> Self {
        Self { signo : sig as i32,
               errno : 0,
               code : 0,
               payload : [0; 116] }
    }
}

const _ : () = assert!(core::mem::size_of::<UserSigAction>() == RT_SIGACTION_SIZE);
const _ : () = assert!(core::mem::size_of::<UserSigInfo>() == 128);

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
struct UserTimeVal {
    sec : isize,
    usec : isize,
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

/// Linux 64-bit `struct rusage`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserRUsage {
    utime : UserTimeVal,
    stime : UserTimeVal,
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

/// Linux `struct utsname`（与 libc 对齐）。
#[repr(C)]
#[derive(Clone, Copy)]
struct UserUtsName {
    sysname : [u8; UTS_LEN],
    nodename : [u8; UTS_LEN],
    release : [u8; UTS_LEN],
    version : [u8; UTS_LEN],
    machine : [u8; UTS_LEN],
    domainname : [u8; UTS_LEN],
}

/// Linux `struct rlimit`（64-bit 下 rlim_t = u64）。
#[repr(C)]
#[derive(Clone, Copy)]
struct UserRLimit {
    cur : u64,
    max : u64,
}

/// Linux 64-bit `struct sysinfo`.
#[repr(C)]
#[derive(Clone, Copy)]
struct UserSysInfo {
    uptime : isize,
    loads : [usize; 3],
    totalram : usize,
    freeram : usize,
    sharedram : usize,
    bufferram : usize,
    totalswap : usize,
    freeswap : usize,
    procs : u16,
    pad : u16,
    totalhigh : usize,
    freehigh : usize,
    mem_unit : u32,
}

const _ : () = assert!(core::mem::size_of::<UserSysInfo>() == 112);
const _ : () = assert!(core::mem::size_of::<UserRUsage>() == 144);
const _ : () = assert!(core::mem::size_of::<UserITimerVal>() == 32);
const RLIM_INFINITY : u64 = !0u64;
const SYSINFO_TOTAL_RAM : usize = wateros_base_config::mm::QEMU_VIRT_PHYS_RAM_SIZE;
const SYSINFO_FREE_RAM : usize = SYSINFO_TOTAL_RAM / 2;
const RUSAGE_CHILDREN : isize = -1;
const RUSAGE_SELF : isize = 0;
const RUSAGE_THREAD : isize = 1;

fn make_uts_field(s : &str) -> [u8; UTS_LEN] {
    let mut buf = [0u8; UTS_LEN];
    let bytes = s.as_bytes();
    let n = bytes.len()
                 .min(UTS_LEN - 1);
    buf[..n].copy_from_slice(&bytes[..n]);
    buf
}

pub(crate) fn sys_yield() -> UserRet {
    task::yield_now();
    UserRet::from_success(0)
}

pub(crate) fn sys_exit(exit_code : isize) -> isize {
    if let Some(task_id) = task::current_task_id() {
        if let Some(process_task) = task::current_process_task_snapshot() {
            reap_exited_member_threads_runtime_resources(process_task.pid);
        }
        if let Some(process_task) = task::process_task_snapshot(task_id) {
            let last_thread = task::process_snapshot(process_task.pid).is_some_and(|process| {
                                                                          process.task_count <= 1
                                                                      });
            if last_thread {
                super::signal::notify_parent_sigchld(process_task.pid);
            }
            super::signal::on_thread_exit(task_id,
                                          process_task.pid
                                                      .raw(),
                                          last_thread);
        }
        if let Some(clear_child_tid) = task::task_clear_child_tid(task_id) {
            let addr = clear_child_tid.user_addr();
            if addr != 0 {
                let clear_result = copy_to_user_struct(addr, &0u32);
                let _ = super::futex::wake_user_addr(addr);
                if let Err(err) = clear_result {
                    log::warn!("[exit] clear_child_tid write failed task_id={} addr={:#x}: {:?}",
                               task_id,
                               addr,
                               err);
                }
            }
        }
        super::robust::robust_exit_cleanup(task_id);
        drop_task_runtime_resources(task_id);
    }
    super::bringup_stats::record_sys_exit();
    task::exit_current(exit_code)
}

pub(crate) fn sys_exit_group(exit_code : isize) -> isize {
    if let Some(task_id) = task::current_task_id() {
        if let Some(clear_child_tid) = task::task_clear_child_tid(task_id) {
            let addr = clear_child_tid.user_addr();
            if addr != 0 {
                let clear_result = copy_to_user_struct(addr, &0u32);
                let _ = super::futex::wake_user_addr(addr);
                if let Err(err) = clear_result {
                    log::warn!("[exit] clear_child_tid write failed task_id={} addr={:#x}: {:?}",
                               task_id,
                               addr,
                               err);
                }
            }
        }
        if let Some(process_task) = task::current_process_task_snapshot() {
            reap_exited_member_threads_runtime_resources(process_task.pid);
            super::signal::notify_parent_sigchld(process_task.pid);
            if let Some(task_ids) = task::task_ids_for_process(process_task.pid) {
                let user_aspace = task::current_task_user_aspace_ptr();
                for sibling in task_ids {
                    if sibling != task_id {
                        super::robust::robust_exit_cleanup(sibling);
                        super::shm::drop_task_attachments(sibling, user_aspace);
                    }
                }
            }
            super::signal::on_thread_exit(task_id,
                                          process_task.pid
                                                      .raw(),
                                          true);
        }
        super::robust::robust_exit_cleanup(task_id);
        drop_task_runtime_resources(task_id);
    }
    task::exit_group_current(exit_code)
}

pub(crate) fn sys_getpid() -> UserRet {
    task::current_process_task_snapshot().map(|snapshot| UserRet::from_success(snapshot.pid.raw()))
                                         .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

pub(crate) fn sys_getppid() -> UserRet {
    let snapshot = match task::current_process_snapshot() {
        Some(snapshot) => snapshot,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    UserRet::from_success(snapshot.parent_pid
                                  .map(|pid| pid.raw())
                                  .unwrap_or(ORPHAN_PARENT_PID))
}

pub(crate) fn sys_gettid() -> UserRet {
    task::current_thread_id().map(|tid| UserRet::from_success(tid.raw()))
                             .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

pub(crate) fn sys_setsid() -> UserRet {
    task::current_process_task_snapshot().map(|snapshot| UserRet::from_success(snapshot.pid.raw()))
                                         .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

/// `setpgid(2)`：bring-up 最小实现；尚未维护真实 pgid，仅校验常见自调用语义。
pub(crate) fn sys_setpgid(args : SyscallArgs) -> UserRet {
    let pid_arg = args.arg(0) as i32;
    let pgid_arg = args.arg(1) as i32;

    if pgid_arg < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let current_pid = match task::current_process_task_snapshot() {
        Some(snapshot) => i32::try_from(snapshot.pid.raw()).unwrap_or(i32::MAX),
        None => return UserRet::from_error(ErrNo::ESRCH),
    };

    let target_pid = if pid_arg == 0 { current_pid } else { pid_arg };
    if target_pid != current_pid {
        return UserRet::from_error(ErrNo::ESRCH);
    }

    let _new_pgid = if pgid_arg == 0 { target_pid } else { pgid_arg };
    UserRet::from_success(0)
}

pub(crate) fn sys_set_tid_address(args : SyscallArgs) -> UserRet {
    let task_id = match task::current_task_id() {
        Some(task_id) => task_id,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let tid = match task::current_thread_id() {
        Some(tid) => tid,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let user_addr = args.arg(0);
    let clear_child_tid = if user_addr == 0 {
        None
    } else {
        Some(task::TaskClearTid::new(user_addr))
    };
    let _ = task::set_task_clear_child_tid(task_id, clear_child_tid);
    UserRet::from_success(tid.raw())
}

pub(crate) fn sys_getrandom(args : SyscallArgs) -> UserRet {
    let buf_ptr = args.arg(0);
    let buflen = args.arg(1);
    let flags = args.arg(2);

    if flags & !GETRANDOM_ALLOWED_FLAGS != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if buflen == 0 {
        return UserRet::from_success(0);
    }
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let tid = task::current_task_id().unwrap_or(0);
    let mut state = random_seed(buf_ptr, buflen, flags, tid);
    let mut written = 0usize;
    let mut chunk = [0u8; 64];
    while written < buflen {
        let n = core::cmp::min(chunk.len(), buflen - written);
        fill_pseudo_random(&mut state, &mut chunk[..n]);
        match copy_to_user(buf_ptr + written, &chunk[..n]) {
            Ok(copied) if copied == n => written += n,
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
    }

    UserRet::from_success(written)
}

pub(crate) fn sys_sysinfo(args : SyscallArgs) -> UserRet {
    let info_ptr = args.arg(0);
    if info_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let info = UserSysInfo { uptime : task::current_tick() as isize,
                             loads : [0; 3],
                             totalram : SYSINFO_TOTAL_RAM,
                             freeram : SYSINFO_FREE_RAM,
                             sharedram : 0,
                             bufferram : 0,
                             totalswap : 0,
                             freeswap : 0,
                             procs : 1,
                             pad : 0,
                             totalhigh : 0,
                             freehigh : 0,
                             mem_unit : 1 };
    match copy_to_user_struct(info_ptr, &info) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

fn random_seed(buf_ptr : usize, buflen : usize, flags : usize, tid : usize) -> u64 {
    let tick = task::current_tick() as u64;
    let mixed = (buf_ptr as u64).rotate_left(17) ^
                (buflen as u64).rotate_left(31) ^
                (flags as u64).rotate_left(7) ^
                (tid as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^
                tick.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    if mixed == 0 {
        0x6A09_E667_F3BC_C909
    } else {
        mixed
    }
}

fn fill_pseudo_random(state : &mut u64, out : &mut [u8]) {
    for byte in out {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        *byte = (x >> 24) as u8;
    }
}

pub(crate) fn sys_rt_sigprocmask(args : SyscallArgs) -> UserRet {
    let how = args.arg(0);
    let set = args.arg(1);
    let oldset = args.arg(2);
    let sigset_size = args.arg(3);

    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match super::signal::ensure_current_signal_state() {
        Ok(snapshot) => snapshot.task_id,
        Err(error) => return UserRet::from_error(error),
    };
    let new_set = if set == 0 {
        None
    } else {
        match copy_from_user_struct::<u64>(set) {
            Ok(bits) => Some(SignalSet::from_bits(bits)),
            Err(e) => return UserRet::from_error(e),
        }
    };
    let old =
        match ipc::signal::with_registry(|registry| registry.update_mask(task_id, how, new_set)) {
            Ok(old) => old,
            Err(SignalError::InvalidHow) => return UserRet::from_error(ErrNo::EINVAL),
            Err(_) => return UserRet::from_error(ErrNo::EINVAL),
        };
    if oldset != 0 {
        if let Err(e) = copy_to_user_struct(oldset, &old.bits()) {
            return UserRet::from_error(e);
        }
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_rt_sigtimedwait(args : SyscallArgs) -> UserRet {
    let set = args.arg(0);
    let info = args.arg(1);
    let timeout = args.arg(2);
    let sigset_size = args.arg(3);

    if set == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match super::signal::ensure_current_signal_state() {
        Ok(snapshot) => snapshot.task_id,
        Err(error) => return UserRet::from_error(error),
    };
    let wait_set = match copy_from_user_struct::<u64>(set) {
        Ok(bits) => SignalSet::from_bits(bits),
        Err(e) => return UserRet::from_error(e),
    };
    let deadline = if timeout == 0 {
        None
    } else {
        let timeout = match copy_from_user_struct::<UserTimespec>(timeout) {
            Ok(timeout)
                if timeout.sec >= 0 && timeout.nsec >= 0 && timeout.nsec < 1_000_000_000 =>
            {
                timeout
            }
            Ok(_) => return UserRet::from_error(ErrNo::EINVAL),
            Err(error) => return UserRet::from_error(error),
        };
        let duration = (timeout.sec as u128).saturating_mul(1_000_000_000)
                                            .saturating_add(timeout.nsec as u128);
        let now = platform::wall_clock::monotonic_ns().unwrap_or(0);
        Some(now.saturating_add(duration))
    };
    let sig = loop {
        if let Some(sig) =
            ipc::signal::with_registry(|registry| registry.take_pending(task_id, wait_set))
        {
            break sig;
        }
        let ticks = match deadline {
            Some(deadline) => {
                let now = platform::wall_clock::monotonic_ns().unwrap_or(deadline);
                if now >= deadline {
                    return UserRet::from_error(ErrNo::EAGAIN);
                }
                let tick_ns =
                    (wateros_base_config::task::SCHED_TIMER_PERIOD_MS as u128) * 1_000_000;
                u64::try_from((deadline - now).saturating_add(tick_ns - 1) / tick_ns)
                    .unwrap_or(u64::MAX)
                    .max(1)
            }
            None => u64::MAX,
        };
        let _ =
            ipc::signal::with_registry(|registry| registry.begin_signal_wait(task_id, wait_set));
        let wait_queue = task::wait_queue::WaitQueue::new();
        let still_waiting = || {
            ipc::signal::with_registry(|registry| {
                registry.pending(task_id)
                        .map(|pending| {
                            pending.intersection(wait_set)
                                   .is_empty()
                        })
                        .unwrap_or(false)
            })
        };
        let wait_result = if deadline.is_some() {
            wait_queue.wait_current_while_for_ticks(ticks, still_waiting)
        } else {
            wait_queue.wait_current_while(still_waiting)
        };
        let _ = ipc::signal::with_registry(|registry| registry.end_signal_wait(task_id));
        if wait_result == task::TaskWaitResult::Interrupted {
            if let Some(sig) =
                ipc::signal::with_registry(|registry| registry.take_pending(task_id, wait_set))
            {
                break sig;
            }
            return UserRet::from_error(ErrNo::EINTR);
        }
    };
    if info != 0 {
        let siginfo = UserSigInfo::for_signal(sig);
        if let Err(e) = copy_to_user_struct(info, &siginfo) {
            return UserRet::from_error(e);
        }
    }
    UserRet::from_success(sig)
}

pub(crate) fn sys_rt_sigaction(args : SyscallArgs) -> UserRet {
    let sig = args.arg(0);
    let act = args.arg(1);
    let oldact = args.arg(2);
    let sigset_size = args.arg(3);

    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match super::signal::ensure_current_signal_state() {
        Ok(snapshot) => snapshot.task_id,
        Err(error) => return UserRet::from_error(error),
    };
    let old = match ipc::signal::with_registry(|registry| registry.get_action(task_id, sig)) {
        Ok(old) => old,
        Err(_) => return UserRet::from_error(ErrNo::EINVAL),
    };
    if oldact != 0 {
        let user_old = UserSigAction { handler : old.handler,
                                       flags : old.flags,
                                       mask : old.mask.bits() };
        if let Err(e) = copy_to_user_struct(oldact, &user_old) {
            return UserRet::from_error(e);
        }
    }
    if act != 0 {
        let user_action = match copy_from_user_struct::<UserSigAction>(act) {
            Ok(action) => action,
            Err(e) => return UserRet::from_error(e),
        };
        let action = SignalAction { handler : user_action.handler,
                                    flags : user_action.flags,
                                    restorer : 0,
                                    mask : SignalSet::from_bits(user_action.mask) };
        match ipc::signal::with_registry(|registry| registry.set_action(task_id, sig, action)) {
            Ok(_) => {}
            Err(_) => return UserRet::from_error(ErrNo::EINVAL),
        }
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_times(args : SyscallArgs) -> UserRet {
    let tms_ptr = args.arg(0);
    if tms_ptr != 0 {
        let snapshot = match task::current_task_snapshot() {
            Some(snapshot) => snapshot,
            None => return UserRet::from_error(ErrNo::ESRCH),
        };
        let tms = UserTms { utime : snapshot.stats
                                            .tick_count
                                    as isize,
                            stime : 0,
                            cutime : 0,
                            cstime : 0 };
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
            usage.utime.sec = ticks / 100;
            usage.utime.usec = (ticks % 100) * 10_000;
        }
        RUSAGE_CHILDREN => {}
        _ => return UserRet::from_error(ErrNo::EINVAL),
    }
    match copy_to_user_struct(usage_ptr, &usage) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
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
    let snapshot = match super::signal::ensure_current_signal_state() {
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
    let old = match ipc::signal::with_registry(|registry| {
              registry.set_timer(snapshot.pid.raw(), which, spec, now)
          }) {
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
    let snapshot = match super::signal::ensure_current_signal_state() {
        Ok(snapshot) => snapshot,
        Err(error) => return UserRet::from_error(error),
    };
    let now = match platform::wall_clock::monotonic_ns() {
        Ok(now) => now,
        Err(_) => return UserRet::from_error(ErrNo::EIO),
    };
    let spec = match ipc::signal::with_registry(|registry| {
              registry.get_timer(snapshot.pid.raw(), which, now)
          }) {
        Ok(spec) => spec,
        Err(_) => return UserRet::from_error(ErrNo::EINVAL),
    };
    match copy_to_user_struct(current_value, &timer_spec_to_user(spec)) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
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

/// 信号终止进程的 wait(2) 编码：负值表示被信号杀死，低 7 位为信号号，bit7 为 core dump。
pub(crate) fn signal_terminate_exit_code(signal : usize, task_id : usize) -> isize {
    let mut status = (signal & 0x7f) as isize;
    if let Some(snapshot) = task::process_task_snapshot(task_id) {
        if task::process_resource_limit(snapshot.pid, RLIMIT_CORE)
            .map(|limit| limit.cur > 0)
            .unwrap_or(false)
        {
            status |= 0x80;
        }
    }
    -status
}

fn write_exit_code(exit_code_ptr : usize, exit_code : isize) -> Result<(), ErrNo> {
    if exit_code_ptr == 0 {
        return Ok(());
    }
    let wait_status = if exit_code < 0 {
        (-exit_code) as i32
    } else {
        ((exit_code as i32) & 0xFF) << 8
    };
    copy_to_user_struct(exit_code_ptr, &wait_status)
}

fn drop_exited_task_resources(exited : &task::ExitedTask) {
    let aspace = exited.trap_frame
                       .as_ref()
                       .map(|frame| frame.user_aspace_ptr())
                       .unwrap_or(0);
    drop_task_runtime_resources_with_aspace(exited.id, aspace);
}

fn drop_task_runtime_resources(task_id : task::TaskId) {
    let aspace = if task::current_task_id() == Some(task_id) {
        task::current_task_user_aspace_ptr()
    } else {
        0
    };
    drop_task_runtime_resources_with_aspace(task_id, aspace);
}

fn drop_task_runtime_resources_with_aspace(task_id : task::TaskId, aspace : usize) {
    super::shm::drop_task_attachments(task_id, aspace);
    vfs::cwd::drop_task_cwd(task_id);
    vfs::fd::drop_task_fd_table(task_id);
    crate::socket_fd::drop_task(task_id);
    cred::drop_task_cred(task_id);
}

pub(crate) fn reap_exited_member_threads_runtime_resources(pid : task::ProcessId) {
    let aspace = task::process_snapshot(pid)
        .and_then(|process| process.address_space)
        .map(|address_space| address_space.user_aspace_ptr())
        .unwrap_or(0);
    let reaped = task::reap_exited_member_threads(pid);
    super::bringup_stats::record_reap_member_threads(reaped.len());
    for exited in reaped {
        drop_reaped_task_runtime_resources(exited.id, aspace);
    }
}

pub(crate) fn drop_reaped_task_runtime_resources(task_id : task::TaskId, aspace : usize) {
    super::robust::drop_robust_state(task_id);
    super::signal::drop_thread_state(task_id);
    drop_task_runtime_resources_with_aspace(task_id, aspace);
}

fn finish_wait_process_result(pid : task::ProcessId,
                              exited_tasks : Vec<task::ExitedTask>,
                              exit_code_ptr : usize)
                              -> UserRet {
    let Some(status_task) = exited_tasks.first() else {
        return UserRet::from_error(ErrNo::ECHILD);
    };
    match write_exit_code(exit_code_ptr, status_task.exit_code) {
        Ok(()) => {
            for exited in &exited_tasks {
                drop_exited_task_resources(exited);
            }
            UserRet::from_success(pid.raw())
        }
        Err(e) => UserRet::from_error(e),
    }
}

/// `waitpid`/`wait4` 早期语义：维护最小父子关系并阻塞等待子任务退出。
///
/// `WUNTRACED`/`WCONTINUED` 目前没有 stop/continue 状态可报告，按 no-op 接受以
/// 兼容 busybox shell 的 wait4 调用。
pub(crate) fn sys_waitpid(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let exit_code_ptr = args.arg(1);
    let options = args.arg(2);
    let nohang = (options & WNOHANG) != 0;
    if options & !(WNOHANG | WAITPID_IGNORED_OPTIONS) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let current_process = match task::current_process_snapshot() {
        Some(process) => process,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    if pid == -1 {
        loop {
            if let Some(child) = task::find_exited_child_process(current_process.pid) {
                let Some(exited) = task::reap_exited_process(child.pid) else {
                    return UserRet::from_error(ErrNo::ECHILD);
                };
                return finish_wait_process_result(child.pid, exited, exit_code_ptr);
            }
            if !task::has_child_process(current_process.pid) {
                return UserRet::from_error(ErrNo::ECHILD);
            }
            if nohang {
                return UserRet::from_success(0);
            }
            if waitpid_wait_for_child(current_process.pid) == task::TaskWaitResult::Interrupted {
                return UserRet::from_error(ErrNo::EINTR);
            }
        }
    }
    if pid <= 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let child_pid = task::ProcessId::from_raw(pid as usize);
    match task::process_snapshot(child_pid) {
        Some(snapshot) if snapshot.parent_pid == Some(current_process.pid) => {}
        Some(_) => return UserRet::from_error(ErrNo::ECHILD),
        None => return UserRet::from_error(ErrNo::ECHILD),
    }
    loop {
        if let Some(exited) = task::reap_exited_process(child_pid) {
            return finish_wait_process_result(child_pid, exited, exit_code_ptr);
        }
        if task::process_snapshot(child_pid).is_none() {
            return UserRet::from_error(ErrNo::ECHILD);
        }
        if nohang {
            return UserRet::from_success(0);
        }
        if waitpid_wait_for_child(current_process.pid) == task::TaskWaitResult::Interrupted {
            return UserRet::from_error(ErrNo::EINTR);
        }
    }
}

/// 利用 ChildExit wait queue 事件驱动等待，替代原有的轮询 sleep。
/// `wait_on_while` 的 condition 返回 `true` 才阻塞，返回 `false` 不阻塞。
/// 所以「有子进程且没有子进程退出」→ `true` → 阻塞等待。
fn waitpid_wait_for_child(parent_pid : task::ProcessId) -> task::TaskWaitResult {
    let Some(leader) = task::leader_task_for_process(parent_pid) else {
        return task::TaskWaitResult::Woken;
    };
    let handle = task::TaskWaitHandle::for_child_exit(leader);
    task::wait_on_while(handle, || {
        task::has_child_process(parent_pid) && task::find_exited_child_process(parent_pid).is_none()
    })
}

/// `uname(buf)` — 返回系统信息。
pub(crate) fn sys_uname(args : SyscallArgs) -> UserRet {
    let buf_ptr = args.arg(0);
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    #[cfg(target_arch = "riscv64")]
    let machine = "riscv64";
    #[cfg(target_arch = "loongarch64")]
    let machine = "loongarch64";
    #[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
    let machine = "unknown";
    let uts = UserUtsName { sysname : make_uts_field("WaterOS"),
                            nodename : make_uts_field("wateros"),
                            release : make_uts_field("5.15.0"),
                            version : make_uts_field("WaterOS #1 SMP"),
                            machine : make_uts_field(machine),
                            domainname : make_uts_field("") };
    match copy_to_user_struct(buf_ptr, &uts) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

/// `prctl(option, arg2, arg3, arg4, arg5)` —
/// 进程控制（stub，仅支持常见无操作选项）。
pub(crate) fn sys_prctl(args : SyscallArgs) -> UserRet {
    let option = args.arg(0);
    match option {
        PR_SET_NAME => {
            // 当前不存储线程名，仅无操作返回成功
            UserRet::from_success(0)
        }
        PR_GET_NAME => {
            // 返回空字符串作为线程名
            let name_ptr = args.arg(1);
            if name_ptr == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let name = [0u8; 16];
            match copy_to_user_struct(name_ptr, &name) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        PR_SET_NO_NEW_PRIVS => UserRet::from_success(0),
        PR_CAPBSET_READ => {
            // 所有 capability 均不可用
            UserRet::from_error(ErrNo::EINVAL)
        }
        PR_CAPBSET_DROP => UserRet::from_error(ErrNo::EINVAL),
        _ => UserRet::from_error(ErrNo::ENOSYS),
    }
}

fn default_rlimit(resource : usize) -> UserRLimit {
    match resource {
        RLIMIT_STACK => UserRLimit { cur : 8 * 1024 * 1024,
                                     max : 8 * 1024 * 1024 },
        RLIMIT_NOFILE => UserRLimit { cur : 1024,
                                      max : 1024 },
        RLIMIT_DATA => UserRLimit { cur : RLIM_INFINITY,
                                    max : RLIM_INFINITY },
        RLIMIT_AS => UserRLimit { cur : RLIM_INFINITY,
                                  max : RLIM_INFINITY },
        RLIMIT_CORE => UserRLimit { cur : 0, max : 0 },
        RLIMIT_MEMLOCK => UserRLimit { cur : 64 * 1024,
                                       max : 64 * 1024 },
        RLIMIT_NPROC => UserRLimit { cur : 1024,
                                     max : 1024 },
        _ => UserRLimit { cur : RLIM_INFINITY,
                          max : RLIM_INFINITY },
    }
}

fn current_process_rlimit(resource : usize) -> UserRLimit {
    let default = default_rlimit(resource);
    let Some(pid) = task::current_process_task_snapshot().map(|snapshot| snapshot.pid) else {
        return default;
    };
    task::process_resource_limit(pid, resource).map(|limit| UserRLimit { cur : limit.cur,
                                                                         max : limit.max })
                                               .unwrap_or(default)
}

fn apply_process_rlimit(resource : usize, limit : UserRLimit) -> Result<(), ErrNo> {
    let Some(pid) = task::current_process_task_snapshot().map(|snapshot| snapshot.pid) else {
        return Err(ErrNo::ESRCH);
    };
    task::set_process_resource_limit(pid,
                                     resource,
                                     ResourceLimit { cur : limit.cur,
                                                     max : limit.max })
        .map_err(|err| match err {
            SetResourceLimitError::InvalidArgument => ErrNo::EINVAL,
        })
}

/// `getrlimit(resource, rlim)` — 获取资源限制。
pub(crate) fn sys_getrlimit(args : SyscallArgs) -> UserRet {
    let resource = args.arg(0);
    let rlim_ptr = args.arg(1);
    if rlim_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let rlim = current_process_rlimit(resource);
    match copy_to_user_struct(rlim_ptr, &rlim) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

/// `setrlimit(resource, rlim)` — 设置资源限制。
pub(crate) fn sys_setrlimit(args : SyscallArgs) -> UserRet {
    let resource = args.arg(0);
    let rlim_ptr = args.arg(1);
    if rlim_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let rlim = match copy_from_user_struct::<UserRLimit>(rlim_ptr) {
        Ok(rlim) => rlim,
        Err(e) => return UserRet::from_error(e),
    };
    match apply_process_rlimit(resource, rlim) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

/// `umask(mask)` — 设置文件创建权限掩码并返回旧值。
pub(crate) fn sys_umask(args : SyscallArgs) -> UserRet {
    let new_mask = args.arg(0) & 0o777;
    let old_mask = CURRENT_UMASK.swap(new_mask, Ordering::SeqCst);
    UserRet::from_success(old_mask)
}

/// `prlimit64(pid, resource, new_limit, old_limit)` — 查询/设置当前进程资源限制。
pub(crate) fn sys_prlimit64(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0);
    let resource = args.arg(1);
    let new_limit = args.arg(2);
    let old_limit = args.arg(3);

    if pid != 0 {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    if old_limit != 0 {
        let rlim = current_process_rlimit(resource);
        if let Err(e) = copy_to_user_struct(old_limit, &rlim) {
            return UserRet::from_error(e);
        }
    }
    if new_limit != 0 {
        let rlim = match copy_from_user_struct::<UserRLimit>(new_limit) {
            Ok(rlim) => rlim,
            Err(e) => return UserRet::from_error(e),
        };
        match apply_process_rlimit(resource, rlim) {
            Ok(()) => {}
            Err(e) => return UserRet::from_error(e),
        }
    }
    UserRet::from_success(0)
}
