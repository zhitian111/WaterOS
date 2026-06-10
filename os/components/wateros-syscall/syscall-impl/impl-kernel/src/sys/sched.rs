//! `sched_*` 系统调用：参数解析与用户拷贝；语义委托 [`task`] 调度原语。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use task::{SchedError, SchedParam, SchedPolicy};

use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user, copy_to_user_struct};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserSchedParam {
    sched_priority: i32,
}

fn sched_err_to_errno(err: SchedError) -> ErrNo {
    match err {
        SchedError::InvalidArg => ErrNo::EINVAL,
        SchedError::NoSuchTask => ErrNo::ESRCH,
        SchedError::NotPermitted => ErrNo::EPERM,
    }
}

fn policy_from_arg(raw: isize) -> Result<SchedPolicy, ErrNo> {
    SchedPolicy::from_linux_raw(raw as i32).ok_or(ErrNo::EINVAL)
}

/// `sched_setparam(pid, param)`。
pub(crate) fn sys_sched_setparam(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let param_ptr = args.arg(1);
    if param_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let user_param = match copy_from_user_struct::<UserSchedParam>(param_ptr) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(e),
    };
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let param = SchedParam {
        priority: user_param.sched_priority,
    };
    match task::set_param(task_id, param) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(sched_err_to_errno(e)),
    }
}

/// `sched_setscheduler(pid, policy, param)`。
pub(crate) fn sys_sched_setscheduler(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let policy_raw = args.arg(1) as isize;
    let param_ptr = args.arg(2);
    if param_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let policy = match policy_from_arg(policy_raw) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(e),
    };
    let user_param = match copy_from_user_struct::<UserSchedParam>(param_ptr) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(e),
    };
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let param = SchedParam {
        priority: user_param.sched_priority,
    };
    match task::set_scheduler(task_id, policy, param) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(sched_err_to_errno(e)),
    }
}

/// `sched_getscheduler(pid)`。
pub(crate) fn sys_sched_getscheduler(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    match task::get_scheduler(task_id) {
        Ok(policy) => UserRet::from_success(policy as isize as usize),
        Err(e) => UserRet::from_error(sched_err_to_errno(e)),
    }
}

/// `sched_getparam(pid, param)`。
pub(crate) fn sys_sched_getparam(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let param_ptr = args.arg(1);
    if param_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let param = match task::get_param(task_id) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let user_param = UserSchedParam {
        sched_priority: param.priority,
    };
    match copy_to_user_struct(param_ptr, &user_param) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

/// `sched_setaffinity(pid, cpusetsize, mask)`。
pub(crate) fn sys_sched_setaffinity(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let cpusetsize = args.arg(1);
    let mask_ptr = args.arg(2);
    if mask_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if let Err(e) = task::validate_cpu_affinity_buf_len(cpusetsize) {
        return UserRet::from_error(sched_err_to_errno(e));
    }
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let mut mask = Vec::with_capacity(cpusetsize);
    mask.resize(cpusetsize, 0);
    if let Err(e) = copy_from_user(&mut mask, mask_ptr) {
        return UserRet::from_error(e);
    }
    match task::set_affinity(task_id, &mask) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(sched_err_to_errno(e)),
    }
}

/// `sched_getaffinity(pid, cpusetsize, mask)`。
pub(crate) fn sys_sched_getaffinity(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let cpusetsize = args.arg(1);
    let mask_ptr = args.arg(2);
    if mask_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if let Err(e) = task::validate_cpu_affinity_buf_len(cpusetsize) {
        return UserRet::from_error(sched_err_to_errno(e));
    }
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    if task::get_scheduler(task_id).is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    let mut buf = Vec::with_capacity(cpusetsize);
    buf.resize(cpusetsize, 0);
    task::fill_cpu_affinity_mask(&mut buf);
    match copy_to_user(mask_ptr, &buf) {
        Ok(n) if n == buf.len() => UserRet::from_success(task::cpu_affinity_ret_bytes()),
        Ok(_) => UserRet::from_error(ErrNo::EFAULT),
        Err(e) => UserRet::from_error(e),
    }
}
