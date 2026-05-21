//! 任务相关系统调用：`yield`、`exit`、`waitpid`、`getpid`/`getppid`/`gettid`、
//! `gettimeofday`、`clock_gettime`、`times`、`nanosleep`、`uname`、`prctl`、
//! `getrlimit`/`setrlimit`。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const ORPHAN_PARENT_PID : usize = 1;
const WNOHANG : usize = 1;
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

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimeVal {
    sec : isize,
    usec : isize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTms {
    utime : isize,
    stime : isize,
    cutime : isize,
    cstime : isize,
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

const RLIM_INFINITY : u64 = !0u64;

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

pub(crate) fn sys_exit(exit_code : isize) -> isize { task::exit_current(exit_code) }

pub(crate) fn sys_getpid() -> UserRet {
    task::current_task_id().map(UserRet::from_success)
                           .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

pub(crate) fn sys_getppid() -> UserRet {
    let snapshot = match task::current_task_snapshot() {
        Some(snapshot) => snapshot,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    UserRet::from_success(snapshot.parent_id
                                  .unwrap_or(ORPHAN_PARENT_PID))
}

pub(crate) fn sys_gettid() -> UserRet {
    task::current_task_id().map(UserRet::from_success)
                           .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

pub(crate) fn sys_set_tid_address() -> UserRet {
    // 当前 WaterOS 还没有用户线程组和 clear_child_tid 唤醒语义；按 Linux 约定先返回
    // 当前 tid，地址本身在后续线程/信号工作包中接入 TCB。
    sys_gettid()
}

fn current_tick_for_user_time() -> u64 { task::current_tick().max(1) }

pub(crate) fn sys_gettimeofday(args : SyscallArgs) -> UserRet {
    let timeval_ptr = args.arg(0);
    if timeval_ptr == 0 {
        return UserRet::from_success(0);
    }
    let tick = current_tick_for_user_time();
    let timeval = UserTimeVal { sec : (tick / 1000) as isize,
                                usec : ((tick % 1000) * 1000) as isize };
    match copy_to_user_struct(timeval_ptr, &timeval) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

pub(crate) fn sys_clock_gettime(args : SyscallArgs) -> UserRet {
    let timespec_ptr = args.arg(1);
    if timespec_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let tick = current_tick_for_user_time();
    let timespec = UserTimespec { sec : (tick / 1000) as isize,
                                  nsec : ((tick % 1000) * 1_000_000) as isize };
    match copy_to_user_struct(timespec_ptr, &timespec) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
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

fn write_exit_code(exit_code_ptr : usize, exit_code : isize) -> Result<(), ErrNo> {
    if exit_code_ptr == 0 {
        return Ok(());
    }
    let wait_status = ((exit_code as i32) & 0xFF) << 8;
    copy_to_user_struct(exit_code_ptr, &wait_status)
}

fn finish_wait_result(exited : task::ExitedTask, exit_code_ptr : usize) -> UserRet {
    match write_exit_code(exit_code_ptr, exited.exit_code) {
        Ok(()) => {
            vfs::cwd::drop_task_cwd(exited.id);
            UserRet::from_success(exited.id)
        }
        Err(e) => UserRet::from_error(e),
    }
}

/// `waitpid`/`wait4` 早期语义：维护最小父子关系并阻塞等待子任务退出；暂不解析
/// 除 `WNOHANG` 之外的 options。
pub(crate) fn sys_waitpid(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let exit_code_ptr = args.arg(1);
    let options = args.arg(2);
    let nohang = (options & WNOHANG) != 0;
    if options & !WNOHANG != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let current_task_id = match task::current_task_id() {
        Some(task_id) => task_id,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    if pid == -1 {
        loop {
            if let Some(exited) = task::reap_one_exited_child(current_task_id) {
                return finish_wait_result(exited, exit_code_ptr);
            }
            if !task::has_child(current_task_id) {
                return UserRet::from_error(ErrNo::ECHILD);
            }
            if nohang {
                return UserRet::from_success(0);
            }
            task::wait_on(task::TaskWaitHandle::for_child_exit(current_task_id));
        }
    }
    if pid <= 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let task_id = pid as usize;
    match task::task_snapshot(task_id) {
        Some(snapshot) if snapshot.parent_id == Some(current_task_id) => {}
        Some(_) => return UserRet::from_error(ErrNo::ECHILD),
        None => return UserRet::from_error(ErrNo::ECHILD),
    }
    loop {
        if let Some(exited) = task::reap_exited_task(task_id) {
            return finish_wait_result(exited, exit_code_ptr);
        }
        if task::task_snapshot(task_id).is_none() {
            return UserRet::from_error(ErrNo::ECHILD);
        }
        if nohang {
            return UserRet::from_success(0);
        }
        task::wait_for_task_exit(task_id);
    }
}

/// `nanosleep` 临时映射到一个调度
/// tick；真实时间换算待平台频率语义接入后再替换。
pub(crate) fn sys_nanosleep(args : SyscallArgs) -> UserRet {
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
pub(crate) fn sys_uname(args : SyscallArgs) -> UserRet {
    let buf_ptr = args.arg(0);
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    // 根据目标架构填充 machine 字段
    #[cfg(any(feature = "impl-riscv64", feature = "impl-loongarch64"))]
    let machine = "riscv64";
    #[cfg(not(any(feature = "impl-riscv64", feature = "impl-loongarch64")))]
    let machine = "unknown";
    let uts = UserUtsName { sysname : make_uts_field("WaterOS"),
                            nodename : make_uts_field("wateros"),
                            release : make_uts_field("0.1.0-prototype"),
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

/// `getrlimit(resource, rlim)` — 获取资源限制。
pub(crate) fn sys_getrlimit(args : SyscallArgs) -> UserRet {
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
pub(crate) fn sys_setrlimit(args : SyscallArgs) -> UserRet {
    let _resource = args.arg(0);
    let _rlim_ptr = args.arg(1);
    // 当前不做实际限制，总是返回成功
    UserRet::from_success(0)
}
