//! 资源限制与 umask 系统调用：`getrlimit`、`setrlimit`、`prlimit64`、`umask`。
//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
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
const RLIM_NLIMITS : usize = 16;
const NR_OPEN : u64 = 1024 * 1024;
const RLIM_INFINITY : u64 = !0u64;

fn valid_resource(resource : usize) -> bool { resource < RLIM_NLIMITS }

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

fn process_rlimit_for(pid : task::ProcessId, resource : usize) -> UserRLimit {
    let default = default_rlimit(resource);
    task::process_resource_limit(pid, resource).map(|limit| UserRLimit { cur : limit.cur,
                                                                         max : limit.max })
                                               .unwrap_or(default)
}

fn current_process_rlimit(resource : usize) -> UserRLimit {
    let Some(pid) = task::current_process_task_snapshot().map(|snapshot| snapshot.pid) else {
        return default_rlimit(resource);
    };
    process_rlimit_for(pid, resource)
}

fn apply_process_rlimit_for(pid : task::ProcessId,
                            resource : usize,
                            limit : UserRLimit)
                            -> Result<(), ErrNo> {
    if !valid_resource(resource) {
        return Err(ErrNo::EINVAL);
    }
    if limit.cur > limit.max {
        return Err(ErrNo::EINVAL);
    }
    if resource == RLIMIT_NOFILE && limit.max > NR_OPEN {
        return Err(ErrNo::EPERM);
    }
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

fn apply_process_rlimit(resource : usize, limit : UserRLimit) -> Result<(), ErrNo> {
    let Some(pid) = task::current_process_task_snapshot().map(|snapshot| snapshot.pid) else {
        return Err(ErrNo::ESRCH);
    };
    apply_process_rlimit_for(pid, resource, limit)
}

fn resolve_rlimit_pid(raw_pid : usize) -> Result<task::ProcessId, ErrNo> {
    if raw_pid == 0 {
        return task::current_process_task_snapshot()
            .map(|snapshot| snapshot.pid)
            .ok_or(ErrNo::ESRCH);
    }
    let pid = task::ProcessId::from_raw(raw_pid);
    if task::process_snapshot(pid).is_none() {
        return Err(ErrNo::ESRCH);
    }
    Ok(pid)
}

fn can_set_rlimit(pid : task::ProcessId) -> bool {
    let Some(current) = task::current_process_task_snapshot() else {
        return false;
    };
    if current.pid == pid {
        return true;
    }
    let caller = cred::current_credentials();
    if caller.effective_uid.0 == 0 {
        return true;
    }
    let Some(leader) = task::leader_task_for_process(pid) else {
        return false;
    };
    let target = cred::credentials_for(leader);
    caller.real_uid.0 == target.real_uid.0 ||
    caller.real_uid.0 == target.effective_uid.0 ||
    caller.effective_uid.0 == target.real_uid.0 ||
    caller.effective_uid.0 == target.effective_uid.0
}

/// `getrlimit(resource, rlim)` — 获取资源限制。
pub(crate) fn sys_getrlimit(args : SyscallArgs) -> UserRet {
    let resource = args.arg(0);
    let rlim_ptr = args.arg(1);
    if !valid_resource(resource) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
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
    if !valid_resource(resource) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
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

/// `prlimit64(pid, resource, new_limit, old_limit)` — 查询/设置指定进程资源限制。
pub(crate) fn sys_prlimit64(args : SyscallArgs) -> UserRet {
    let raw_pid = args.arg(0);
    let resource = args.arg(1);
    let new_limit = args.arg(2);
    let old_limit = args.arg(3);

    let pid = match resolve_rlimit_pid(raw_pid) {
        Ok(pid) => pid,
        Err(error) => return UserRet::from_error(error),
    };
    if !valid_resource(resource) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if old_limit != 0 {
        let rlim = process_rlimit_for(pid, resource);
        if let Err(e) = copy_to_user_struct(old_limit, &rlim) {
            return UserRet::from_error(e);
        }
    }
    if new_limit != 0 {
        if !can_set_rlimit(pid) {
            return UserRet::from_error(ErrNo::EPERM);
        }
        let rlim = match copy_from_user_struct::<UserRLimit>(new_limit) {
            Ok(rlim) => rlim,
            Err(e) => return UserRet::from_error(e),
        };
        match apply_process_rlimit_for(pid, resource, rlim) {
            Ok(()) => {}
            Err(e) => return UserRet::from_error(e),
        }
    }
    UserRet::from_success(0)
}
