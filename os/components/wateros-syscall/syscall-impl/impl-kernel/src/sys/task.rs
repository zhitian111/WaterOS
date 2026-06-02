//! 任务相关系统调用：`yield`、`exit`、`waitpid`、`getpid`/`getppid`/`gettid`、
//! `gettimeofday`、`clock_gettime`、`times`、`nanosleep`、`uname`、`prctl`、
//! `getrlimit`/`setrlimit`。

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::{copy_from_user_struct, copy_to_user, copy_to_user_struct};

const ORPHAN_PARENT_PID: usize = 1;
const WNOHANG: usize = 1;
const UTS_LEN: usize = 65;

// prctl 操作码
const PR_SET_NAME: usize = 15;
const PR_GET_NAME: usize = 16;
const PR_SET_NO_NEW_PRIVS: usize = 38;
const PR_CAPBSET_READ: usize = 23;
const PR_CAPBSET_DROP: usize = 24;

// rlimit 资源号
const RLIMIT_STACK: usize = 3;
const RLIMIT_NOFILE: usize = 7;
const RLIMIT_AS: usize = 9;
const RLIMIT_DATA: usize = 2;
const RLIMIT_CORE: usize = 4;
const RLIMIT_MEMLOCK: usize = 8;
const RLIMIT_NPROC: usize = 6;
const ROBUST_LIST_HEAD_SIZE_64: usize = 24;
const RT_SIGSET_SIZE_64: usize = 8;
const RT_SIGACTION_SIZE_MIN: usize = 32;
const GRND_NONBLOCK: usize = 0x0001;
const GRND_RANDOM: usize = 0x0002;
const GRND_INSECURE: usize = 0x0004;
const GETRANDOM_ALLOWED_FLAGS: usize = GRND_NONBLOCK | GRND_RANDOM | GRND_INSECURE;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimeVal {
    sec: isize,
    usec: isize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimespec {
    sec: isize,
    nsec: isize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTms {
    utime: isize,
    stime: isize,
    cutime: isize,
    cstime: isize,
}

/// Linux `struct utsname`（与 libc 对齐）。
#[repr(C)]
#[derive(Clone, Copy)]
struct UserUtsName {
    sysname: [u8; UTS_LEN],
    nodename: [u8; UTS_LEN],
    release: [u8; UTS_LEN],
    version: [u8; UTS_LEN],
    machine: [u8; UTS_LEN],
    domainname: [u8; UTS_LEN],
}

/// Linux `struct rlimit`（64-bit 下 rlim_t = u64）。
#[repr(C)]
#[derive(Clone, Copy)]
struct UserRLimit {
    cur: u64,
    max: u64,
}

const RLIM_INFINITY: u64 = !0u64;

fn make_uts_field(s: &str) -> [u8; UTS_LEN] {
    let mut buf = [0u8; UTS_LEN];
    let bytes = s.as_bytes();
    let n = bytes
        .len()
        .min(UTS_LEN - 1);
    buf[..n].copy_from_slice(&bytes[..n]);
    buf
}

pub(crate) fn sys_yield() -> UserRet {
    task::yield_now();
    UserRet::from_success(0)
}

pub(crate) fn sys_exit(exit_code: isize) -> isize {
    if let Some(task_id) = task::current_task_id() {
        if let Some(clear_child_tid) = task::task_clear_child_tid(task_id) {
            let addr = clear_child_tid.user_addr();
            if addr != 0 {
                let _ = copy_to_user_struct(addr, &0u32);
                let _ = super::futex::wake_user_addr(addr);
            }
        }
    }
    task::exit_current(exit_code)
}

pub(crate) fn sys_exit_group(exit_code: isize) -> isize {
    if let Some(task_id) = task::current_task_id() {
        if let Some(clear_child_tid) = task::task_clear_child_tid(task_id) {
            let addr = clear_child_tid.user_addr();
            if addr != 0 {
                let _ = copy_to_user_struct(addr, &0u32);
                let _ = super::futex::wake_user_addr(addr);
            }
        }
    }
    task::exit_group_current(exit_code)
}

pub(crate) fn sys_getpid() -> UserRet {
    task::current_process_task_snapshot()
        .map(|snapshot| UserRet::from_success(snapshot.pid.raw()))
        .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

pub(crate) fn sys_getppid() -> UserRet {
    let snapshot = match task::current_process_snapshot() {
        Some(snapshot) => snapshot,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    UserRet::from_success(
        snapshot
            .parent_pid
            .map(|pid| pid.raw())
            .unwrap_or(ORPHAN_PARENT_PID),
    )
}

pub(crate) fn sys_gettid() -> UserRet {
    task::current_task_id()
        .map(UserRet::from_success)
        .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

pub(crate) fn sys_set_tid_address(args: SyscallArgs) -> UserRet {
    let tid = match task::current_task_id() {
        Some(tid) => tid,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let user_addr = args.arg(0);
    let clear_child_tid = if user_addr == 0 {
        None
    } else {
        Some(task::TaskClearTid::new(user_addr))
    };
    let _ = task::set_task_clear_child_tid(tid, clear_child_tid);
    UserRet::from_success(tid)
}

pub(crate) fn sys_set_robust_list(args: SyscallArgs) -> UserRet {
    let _head = args.arg(0);
    let len = args.arg(1);
    if len != ROBUST_LIST_HEAD_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_getrandom(args: SyscallArgs) -> UserRet {
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

fn random_seed(buf_ptr: usize, buflen: usize, flags: usize, tid: usize) -> u64 {
    let tick = task::current_tick() as u64;
    let mixed = (buf_ptr as u64).rotate_left(17)
        ^ (buflen as u64).rotate_left(31)
        ^ (flags as u64).rotate_left(7)
        ^ (tid as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ tick.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    if mixed == 0 {
        0x6a09_e667_f3bc_c909
    } else {
        mixed
    }
}

fn fill_pseudo_random(state: &mut u64, out: &mut [u8]) {
    for byte in out {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        *byte = (x >> 24) as u8;
    }
}

pub(crate) fn sys_rt_sigprocmask(args: SyscallArgs) -> UserRet {
    let how = args.arg(0);
    let _set = args.arg(1);
    let oldset = args.arg(2);
    let sigset_size = args.arg(3);

    if how > 2 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if oldset != 0 {
        if copy_to_user_struct(oldset, &0u64).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_rt_sigtimedwait(args: SyscallArgs) -> UserRet {
    let set = args.arg(0);
    let _info = args.arg(1);
    let _timeout = args.arg(2);
    let sigset_size = args.arg(3);

    if set == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    // No per-task pending signal queue yet: report "nothing available" instead
    // of panicking, which is enough for libc probes that sanity-check the call.
    UserRet::from_error(ErrNo::EAGAIN)
}

pub(crate) fn sys_rt_sigaction(args: SyscallArgs) -> UserRet {
    let sig = args.arg(0);
    let _act = args.arg(1);
    let oldact = args.arg(2);
    let sigset_size = args.arg(3);

    if sig == 0 || sig >= 64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if oldact != 0 {
        let zero = [0u8; RT_SIGACTION_SIZE_MIN];
        if copy_to_user(oldact, &zero).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }
    UserRet::from_success(0)
}

fn current_tick_for_user_time() -> u64 {
    task::current_tick().max(1)
}

pub(crate) fn sys_gettimeofday(args: SyscallArgs) -> UserRet {
    let timeval_ptr = args.arg(0);
    if timeval_ptr == 0 {
        return UserRet::from_success(0);
    }
    let tick = current_tick_for_user_time();
    let timeval = UserTimeVal {
        sec: (tick / 1000) as isize,
        usec: ((tick % 1000) * 1000) as isize,
    };
    match copy_to_user_struct(timeval_ptr, &timeval) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

pub(crate) fn sys_clock_gettime(args: SyscallArgs) -> UserRet {
    let timespec_ptr = args.arg(1);
    if timespec_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let tick = current_tick_for_user_time();
    let timespec = UserTimespec {
        sec: (tick / 1000) as isize,
        nsec: ((tick % 1000) * 1_000_000) as isize,
    };
    match copy_to_user_struct(timespec_ptr, &timespec) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

pub(crate) fn sys_times(args: SyscallArgs) -> UserRet {
    let tms_ptr = args.arg(0);
    if tms_ptr != 0 {
        let snapshot = match task::current_task_snapshot() {
            Some(snapshot) => snapshot,
            None => return UserRet::from_error(ErrNo::ESRCH),
        };
        let tms = UserTms {
            utime: snapshot
                .stats
                .tick_count as isize,
            stime: 0,
            cutime: 0,
            cstime: 0,
        };
        if let Err(e) = copy_to_user_struct(tms_ptr, &tms) {
            return UserRet::from_error(e);
        }
    }
    UserRet::from_success(task::current_tick() as usize)
}

fn write_exit_code(exit_code_ptr: usize, exit_code: isize) -> Result<(), ErrNo> {
    if exit_code_ptr == 0 {
        return Ok(());
    }
    let wait_status = ((exit_code as i32) & 0xFF) << 8;
    copy_to_user_struct(exit_code_ptr, &wait_status)
}

/// waitpid 回收用户任务时释放其 Sv39 地址空间（execve 已 drop 的旧 aspace 不在 TCB 中）。
fn drop_exited_user_aspace(exited: &task::ExitedTask) {
    if let Some(trap) = exited.trap_frame {
        let aspace_ptr = trap.user_aspace_ptr();
        if aspace_ptr == 0 {
            return;
        }
        // 勿释放仍在运行的任务地址空间（例如误 reap 当前任务）。
        if task::current_task_user_aspace_ptr() == aspace_ptr {
            return;
        }
        mm::kernel_mm::drop_user_aspace(aspace_ptr);
    }
}

fn drop_exited_task_resources(exited: &task::ExitedTask) {
    vfs::cwd::drop_task_cwd(exited.id);
    vfs::fd::drop_task_fd_table(exited.id);
    cred::drop_task_cred(exited.id);
}

fn finish_wait_process_result(
    pid: task::ProcessId,
    exited_tasks: Vec<task::ExitedTask>,
    exit_code_ptr: usize,
) -> UserRet {
    let Some(status_task) = exited_tasks
        .iter()
        .find(|task| task.id == pid.raw())
        .or_else(|| exited_tasks.first())
    else {
        return UserRet::from_error(ErrNo::ECHILD);
    };
    match write_exit_code(exit_code_ptr, status_task.exit_code) {
        Ok(()) => {
            if let Some(owner) = exited_tasks
                .iter()
                .find(|task| task.id == pid.raw())
                .or_else(|| exited_tasks.first())
            {
                drop_exited_user_aspace(owner);
            }
            for exited in &exited_tasks {
                drop_exited_task_resources(exited);
            }
            UserRet::from_success(pid.raw())
        }
        Err(e) => UserRet::from_error(e),
    }
}

/// `waitpid`/`wait4` 早期语义：维护最小父子关系并阻塞等待子任务退出；暂不解析
/// 除 `WNOHANG` 之外的 options。
pub(crate) fn sys_waitpid(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let exit_code_ptr = args.arg(1);
    let options = args.arg(2);
    let nohang = (options & WNOHANG) != 0;
    if options & !WNOHANG != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let current_process = match task::current_process_snapshot() {
        Some(process) => process,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let current_task_id = task::current_task_id().expect("process snapshot requires current task");
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
            task::wait_on(task::TaskWaitHandle::for_child_exit(
                current_task_id,
            ));
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
    let Some(leader_task_id) = task::leader_task_for_process(child_pid) else {
        return UserRet::from_error(ErrNo::ECHILD);
    };
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
        task::wait_for_task_exit(leader_task_id);
    }
}

/// `nanosleep` 临时映射到一个调度
/// tick；真实时间换算待平台频率语义接入后再替换。
pub(crate) fn sys_nanosleep(args: SyscallArgs) -> UserRet {
    let req_ptr = args.arg(0);
    if req_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let req = match copy_from_user_struct::<UserTimespec>(req_ptr) {
        Ok(req) => req,
        Err(e) => return UserRet::from_error(e),
    };
    if req.sec < 0 || req.nsec < 0 || req.nsec >= 1_000_000_000 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if req.sec == 0 && req.nsec == 0 {
        return UserRet::from_success(0);
    }
    task::sleep_for_ticks(1);
    UserRet::from_success(0)
}

/// `uname(buf)` — 返回系统信息。
pub(crate) fn sys_uname(args: SyscallArgs) -> UserRet {
    let buf_ptr = args.arg(0);
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    // 根据目标架构填充 machine 字段
    #[cfg(any(feature = "impl-riscv64", feature = "impl-loongarch64"))]
    let machine = "riscv64";
    #[cfg(not(any(feature = "impl-riscv64", feature = "impl-loongarch64")))]
    let machine = "unknown";
    let uts = UserUtsName {
        sysname: make_uts_field("WaterOS"),
        nodename: make_uts_field("wateros"),
        release: make_uts_field("5.15.0"),
        version: make_uts_field("WaterOS #1 SMP"),
        machine: make_uts_field(machine),
        domainname: make_uts_field(""),
    };
    match copy_to_user_struct(buf_ptr, &uts) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

/// `prctl(option, arg2, arg3, arg4, arg5)` —
/// 进程控制（stub，仅支持常见无操作选项）。
pub(crate) fn sys_prctl(args: SyscallArgs) -> UserRet {
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

fn default_rlimit(resource: usize) -> UserRLimit {
    match resource {
        RLIMIT_STACK => UserRLimit {
            cur: 8 * 1024 * 1024,
            max: 8 * 1024 * 1024,
        },
        RLIMIT_NOFILE => UserRLimit {
            cur: 1024,
            max: 1024,
        },
        RLIMIT_DATA => UserRLimit {
            cur: RLIM_INFINITY,
            max: RLIM_INFINITY,
        },
        RLIMIT_AS => UserRLimit {
            cur: RLIM_INFINITY,
            max: RLIM_INFINITY,
        },
        RLIMIT_CORE => UserRLimit { cur: 0, max: 0 },
        RLIMIT_MEMLOCK => UserRLimit {
            cur: 64 * 1024,
            max: 64 * 1024,
        },
        RLIMIT_NPROC => UserRLimit {
            cur: 1024,
            max: 1024,
        },
        _ => UserRLimit {
            cur: RLIM_INFINITY,
            max: RLIM_INFINITY,
        },
    }
}

/// `getrlimit(resource, rlim)` — 获取资源限制。
pub(crate) fn sys_getrlimit(args: SyscallArgs) -> UserRet {
    let resource = args.arg(0);
    let rlim_ptr = args.arg(1);
    if rlim_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let rlim = default_rlimit(resource);
    match copy_to_user_struct(rlim_ptr, &rlim) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

/// `setrlimit(resource, rlim)` — 设置资源限制（stub，允许所有软限制降低）。
pub(crate) fn sys_setrlimit(args: SyscallArgs) -> UserRet {
    let _resource = args.arg(0);
    let _rlim_ptr = args.arg(1);
    // 当前不做实际限制，总是返回成功
    UserRet::from_success(0)
}

/// `prlimit64(pid, resource, new_limit, old_limit)` — 最小兼容当前进程资源限制查询。
pub(crate) fn sys_prlimit64(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0);
    let resource = args.arg(1);
    let new_limit = args.arg(2);
    let old_limit = args.arg(3);

    if pid != 0 {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    if new_limit != 0 && copy_from_user_struct::<UserRLimit>(new_limit).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if old_limit != 0 {
        let rlim = default_rlimit(resource);
        if let Err(e) = copy_to_user_struct(old_limit, &rlim) {
            return UserRet::from_error(e);
        }
    }
    UserRet::from_success(0)
}
