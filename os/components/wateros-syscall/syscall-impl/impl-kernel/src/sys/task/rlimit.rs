//! 资源限制与 umask 系统调用：`getrlimit`、`setrlimit`、`prlimit64`、`umask`。
//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use task::{ProcessError, ResourceLimit};

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

// rlimit 资源号
pub(crate) const RLIMIT_CORE : usize = 4;
const RLIMIT_STACK : usize = 3;
const RLIMIT_NOFILE : usize = 7;
const RLIMIT_AS : usize = 9;
const RLIMIT_DATA : usize = 2;
const RLIMIT_MEMLOCK : usize = 8;
const RLIMIT_NPROC : usize = 6;
const RLIM_INFINITY : u64 = !0u64;

/// Linux `struct rlimit`（64-bit 下 rlim_t = u64）。
#[repr(C)]
#[derive(Clone, Copy)]
struct UserRLimit {
    cur : u64,
    max : u64,
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
    task::set_process_resource_limit(
        pid,
        resource,
        ResourceLimit {
            cur: limit.cur,
            max: limit.max,
        },
    )
    .map_err(|err| match err {
        ProcessError::ProcessNotFound | ProcessError::TaskNotFound => ErrNo::ESRCH,
        _ => ErrNo::EINVAL,
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
    let new_mask = (args.arg(0) & 0o777) as u32;
    let Some(pid) = task::current_process_task_snapshot().map(|s| s.pid) else {
        return UserRet::from_error(ErrNo::ESRCH);
    };
    let old_mask = task::process_umask(pid).unwrap_or(0o022);
    let _ = task::set_process_umask(pid, new_mask);
    UserRet::from_success(old_mask as usize)
}

/// 读取当前进程的 umask（文件创建时用于过滤权限位）。
pub(crate) fn current_umask() -> u32 {
    task::current_process_task_snapshot().and_then(|s| task::process_umask(s.pid))
                                         .unwrap_or(0o022)
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
