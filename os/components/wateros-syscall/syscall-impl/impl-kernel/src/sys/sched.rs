//! `sched_*` 系统调用：参数解析与用户拷贝；语义委托 [`task`] 调度原语。

extern crate alloc;

use core::mem::size_of;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use task::{SchedError, SchedParam, SchedPolicy};

use crate::fallible_buf::{try_kbuf, SCHED_CPUSET_MAX};
use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user, copy_to_user_struct};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserSchedParam {
    sched_priority: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserSchedAttr {
    size: u32,
    sched_policy: u32,
    sched_flags: u64,
    sched_nice: i32,
    sched_priority: u32,
    sched_runtime: u64,
    sched_deadline: u64,
    sched_period: u64,
    sched_util_min: u32,
    sched_util_max: u32,
}

const USER_SCHED_ATTR_PRIORITY_END: usize = 24;

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

fn read_user_sched_attr_size(attr_ptr: usize) -> Result<usize, ErrNo> {
    let mut raw_size = [0u8; size_of::<u32>()];
    copy_from_user(&mut raw_size, attr_ptr)?;
    Ok(u32::from_ne_bytes(raw_size) as usize)
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
    if cpusetsize > SCHED_CPUSET_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let mut mask = match try_kbuf(cpusetsize, SCHED_CPUSET_MAX) {
        Ok(buf) => buf,
        Err(err) => return UserRet::from_error(err),
    };
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
    if cpusetsize > SCHED_CPUSET_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    if task::get_scheduler(task_id).is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    let mut buf = match try_kbuf(cpusetsize, SCHED_CPUSET_MAX) {
        Ok(buf) => buf,
        Err(err) => return UserRet::from_error(err),
    };
    task::fill_cpu_affinity_mask(&mut buf);
    match copy_to_user(mask_ptr, &buf) {
        Ok(n) if n == buf.len() => UserRet::from_success(task::cpu_affinity_ret_bytes()),
        Ok(_) => UserRet::from_error(ErrNo::EFAULT),
        Err(e) => UserRet::from_error(e),
    }
}

/// `sched_setattr(pid, attr, flags)`.
pub(crate) fn sys_sched_setattr(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let attr_ptr = args.arg(1);
    let flags = args.arg(2);
    if attr_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if flags != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let user_size = match read_user_sched_attr_size(attr_ptr) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(e),
    };
    if user_size < USER_SCHED_ATTR_PRIORITY_END {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let mut attr = UserSchedAttr::default();
    let copy_len = user_size.min(size_of::<UserSchedAttr>());
    let attr_bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut attr as *mut UserSchedAttr).cast::<u8>(),
            size_of::<UserSchedAttr>(),
        )
    };
    if let Err(e) = copy_from_user(&mut attr_bytes[..copy_len], attr_ptr) {
        return UserRet::from_error(e);
    }

    let policy = match policy_from_arg(attr.sched_policy as isize) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(e),
    };
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let param = SchedParam {
        priority: attr.sched_priority as i32,
    };
    match task::set_scheduler(task_id, policy, param) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(sched_err_to_errno(e)),
    }
}

/// `sched_getattr(pid, attr, size, flags)`.
pub(crate) fn sys_sched_getattr(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let attr_ptr = args.arg(1);
    let user_size = args.arg(2);
    let flags = args.arg(3);
    if attr_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if flags != 0 || user_size < USER_SCHED_ATTR_PRIORITY_END {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let policy = match task::get_scheduler(task_id) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let param = match task::get_param(task_id) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };

    let attr = UserSchedAttr {
        size: size_of::<UserSchedAttr>() as u32,
        sched_policy: policy as u32,
        sched_priority: param.priority as u32,
        ..UserSchedAttr::default()
    };
    let write_len = user_size.min(size_of::<UserSchedAttr>());
    let attr_bytes = unsafe {
        core::slice::from_raw_parts(
            (&attr as *const UserSchedAttr).cast::<u8>(),
            size_of::<UserSchedAttr>(),
        )
    };
    match copy_to_user(attr_ptr, &attr_bytes[..write_len]) {
        Ok(n) if n == write_len => UserRet::from_success(0),
        Ok(_) => UserRet::from_error(ErrNo::EFAULT),
        Err(e) => UserRet::from_error(e),
    }
}

/// `sched_get_priority_max(policy)`。
pub(crate) fn sys_sched_get_priority_max(args: SyscallArgs) -> UserRet {
    let policy = match policy_from_arg(args.arg(0) as isize) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(e),
    };
    let max = match policy {
        SchedPolicy::Other => 0,
        SchedPolicy::Fifo | SchedPolicy::Rr => 99,
    };
    UserRet::from_success(max as isize as usize)
}

/// `sched_get_priority_min(policy)`。
pub(crate) fn sys_sched_get_priority_min(args: SyscallArgs) -> UserRet {
    let policy = match policy_from_arg(args.arg(0) as isize) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(e),
    };
    let min = match policy {
        SchedPolicy::Other => 0,
        SchedPolicy::Fifo | SchedPolicy::Rr => 1,
    };
    UserRet::from_success(min as isize as usize)
}
