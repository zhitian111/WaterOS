//! `sched_*` 系统调用：参数解析与用户拷贝；语义委托 [`task`] 调度原语。
//! 本模块代码由AI完成

extern crate alloc;

use core::mem::size_of;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use task::{CpuMask, SchedError, SchedPolicy};

use crate::user_copy::{copy_from_user, copy_to_user, copy_to_user_struct};

// 本结构代码由AI完成
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserSchedAttr {
    size : u32,
    sched_policy : u32,
    sched_flags : u64,
    sched_nice : i32,
    sched_priority : u32,
    sched_runtime : u64,
    sched_deadline : u64,
    sched_period : u64,
    sched_util_min : u32,
    sched_util_max : u32,
}

/// Linux `SCHED_ATTR_SIZE_VER0`：不含 `sched_util_min`/`sched_util_max` 的最小版本大小。
const SCHED_ATTR_SIZE_VER0 : usize = 48;
const SCHED_OTHER_RAW : isize = 0;
const SCHED_FIFO_RAW : isize = 1;
const SCHED_RR_RAW : isize = 2;
const SCHED_BATCH_RAW : isize = 3;
const SCHED_IDLE_RAW : isize = 5;
const SCHED_DEADLINE_RAW : isize = 6;

fn sched_err_to_errno(err : SchedError) -> ErrNo {
    match err {
        SchedError::InvalidArg => ErrNo::EINVAL,
        SchedError::NoSuchTask => ErrNo::ESRCH,
        SchedError::NotPermitted => ErrNo::EPERM,
    }
}

fn policy_from_arg(raw : isize) -> Result<SchedPolicy, ErrNo> {
    SchedPolicy::from_linux_raw(raw as i32).ok_or(ErrNo::EINVAL)
}

/// `sched_setscheduler`/`sched_setattr` 策略参数转换。
///
/// 仅接受 WaterOS 实际支持的策略（Other/Fifo/Rr），
/// 不支持的策略（BATCH/IDLE/DEADLINE）返回 `EINVAL`，避免静默转换。
fn policy_from_setscheduler_arg(raw : isize) -> Result<SchedPolicy, ErrNo> { policy_from_arg(raw) }

fn read_user_sched_attr_size(attr_ptr : usize) -> Result<usize, ErrNo> {
    let mut raw_size = [0u8; size_of::<u32>()];
    copy_from_user(&mut raw_size, attr_ptr)?;
    Ok(u32::from_ne_bytes(raw_size) as usize)
}

/// `getcpu(cpu, node, tcache)`：返回执行当前 syscall 的逻辑 CPU 与 NUMA node。
///
/// QEMU 支持的平台目前都是单 NUMA node；Linux 已忽略第三个 cache 参数。
pub(crate) fn sys_getcpu(args : SyscallArgs) -> UserRet {
    let cpu_ptr = args.arg(0);
    let node_ptr = args.arg(1);
    let cpu = platform::arch::cpu::current_cpu_id().raw() as u32;
    let node = 0u32;

    if cpu_ptr != 0 {
        if let Err(error) = copy_to_user_struct(cpu_ptr, &cpu) {
            return UserRet::from_error(error);
        }
    }
    if node_ptr != 0 {
        if let Err(error) = copy_to_user_struct(node_ptr, &node) {
            return UserRet::from_error(error);
        }
    }
    UserRet::from_success(0)
}


pub(crate) fn sys_sched_setparam(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let param_ptr = args.arg(1);
    if param_ptr == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let mut raw_priority = [0u8; 4];
    if let Err(e) = copy_from_user(&mut raw_priority, param_ptr) {
        return UserRet::from_error(e);
    }
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    match task::set_param(task_id,
                          i32::from_ne_bytes(raw_priority))
    {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(sched_err_to_errno(e)),
    }
}

/// `sched_setscheduler(pid, policy, param)`。
// 本方法代码由AI完成
pub(crate) fn sys_sched_setscheduler(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let policy_raw = args.arg(1) as isize;
    let param_ptr = args.arg(2);
    if param_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let policy = match policy_from_setscheduler_arg(policy_raw) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(e),
    };
    let mut raw_priority = [0u8; 4];
    if let Err(e) = copy_from_user(&mut raw_priority, param_ptr) {
        return UserRet::from_error(e);
    }
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    match task::set_scheduler_policy(task_id,
                                     policy,
                                     i32::from_ne_bytes(raw_priority))
    {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(sched_err_to_errno(e)),
    }
}

/// `sched_getscheduler(pid)`。
// 本方法代码由AI完成
pub(crate) fn sys_sched_getscheduler(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    match task::get_scheduler_policy(task_id) {
        Ok(policy) => UserRet::from_success(policy as isize as usize),
        Err(e) => UserRet::from_error(sched_err_to_errno(e)),
    }
}

/// `sched_getparam(pid, param)`。
// 本方法代码由AI完成
pub(crate) fn sys_sched_getparam(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let param_ptr = args.arg(1);
    if param_ptr == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let priority = match task::get_param(task_id) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    if let Err(e) = copy_to_user(param_ptr, &priority.to_ne_bytes()) {
        return UserRet::from_error(e);
    }
    UserRet::from_success(0)
}

/// `sched_setaffinity(pid, cpusetsize, mask)`。
// 本方法代码由AI完成
pub(crate) fn sys_sched_setaffinity(args : SyscallArgs) -> UserRet {
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
    if !can_change_affinity(task_id) {
        return UserRet::from_error(ErrNo::EPERM);
    }
    let mut mask = [0u8; 8];
    if let Err(e) = copy_from_user(&mut mask, mask_ptr) {
        return UserRet::from_error(e);
    }
    let mask = match CpuMask::try_from_le_bytes(&mask) {
        Some(mask) => mask,
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    match task::set_affinity(task_id, mask) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(sched_err_to_errno(e)),
    }
}

fn can_change_affinity(target : task::TaskId) -> bool {
    let caller = cred::current_credentials();
    if caller.effective_uid
             .0 ==
       0
    {
        return true;
    }
    let target = cred::credentials_for(target);
    caller.real_uid.0 == target.real_uid.0 ||
    caller.real_uid.0 ==
    target.effective_uid
          .0 ||
    caller.effective_uid
          .0 ==
    target.real_uid.0 ||
    caller.effective_uid
          .0 ==
    target.effective_uid
          .0
}

/// `sched_getaffinity(pid, cpusetsize, mask)`。
// 本方法代码由AI完成
pub(crate) fn sys_sched_getaffinity(args : SyscallArgs) -> UserRet {
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
    if task::get_scheduler_policy(task_id).is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    let mut buf = [0u8; 8];
    let affinity = match task::get_affinity(task_id) {
        Ok(mask) => mask,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let affinity = CpuMask::from_bits(affinity.bits() & task::online_cpu_mask().bits());
    affinity.write_le_bytes(&mut buf);
    match copy_to_user(mask_ptr, &buf) {
        Ok(n) if n == buf.len() => UserRet::from_success(task::cpu_affinity_ret_bytes()),
        Ok(_) => UserRet::from_error(ErrNo::EFAULT),
        Err(e) => UserRet::from_error(e),
    }
}

/// `sched_setattr(pid, attr, flags)`.
// 本方法代码由AI完成
pub(crate) fn sys_sched_setattr(args : SyscallArgs) -> UserRet {
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
    if user_size < SCHED_ATTR_SIZE_VER0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let mut attr = UserSchedAttr::default();
    let copy_len = user_size.min(size_of::<UserSchedAttr>());
    let attr_bytes = unsafe {
        core::slice::from_raw_parts_mut((&mut attr as *mut UserSchedAttr).cast::<u8>(),
                                        size_of::<UserSchedAttr>())
    };
    if let Err(e) = copy_from_user(&mut attr_bytes[..copy_len], attr_ptr) {
        return UserRet::from_error(e);
    }

    let policy = match policy_from_setscheduler_arg(attr.sched_policy as isize) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(e),
    };
    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    match task::set_scheduler_policy(task_id,
                                     policy,
                                     attr.sched_priority as i32)
    {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(sched_err_to_errno(e)),
    }
}

/// `sched_getattr(pid, attr, size, flags)`.
// 本方法代码由AI完成
pub(crate) fn sys_sched_getattr(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let attr_ptr = args.arg(1);
    let user_size = args.arg(2);
    let flags = args.arg(3);
    if attr_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if flags != 0 || user_size < SCHED_ATTR_SIZE_VER0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let task_id = match task::resolve_sched_pid(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let policy = match task::get_scheduler_policy(task_id) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let priority = match task::get_param(task_id) {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };

    let nice = match task::get_nice(task_id) {
        Ok(nice) => nice as i32,
        Err(e) => return UserRet::from_error(sched_err_to_errno(e)),
    };
    let attr = UserSchedAttr { size : size_of::<UserSchedAttr>() as u32,
                               sched_policy : policy as u32,
                               sched_nice : nice,
                               sched_priority : priority as u32,
                               ..UserSchedAttr::default() };
    let write_len = user_size.min(size_of::<UserSchedAttr>());
    let attr_bytes = unsafe {
        core::slice::from_raw_parts((&attr as *const UserSchedAttr).cast::<u8>(),
                                    size_of::<UserSchedAttr>())
    };
    match copy_to_user(attr_ptr, &attr_bytes[..write_len]) {
        Ok(n) if n == write_len => UserRet::from_success(0),
        Ok(_) => UserRet::from_error(ErrNo::EFAULT),
        Err(e) => UserRet::from_error(e),
    }
}

/// `sched_get_priority_max(policy)`。
// 本方法代码由AI完成
pub(crate) fn sys_sched_get_priority_max(args : SyscallArgs) -> UserRet {
    let max = match args.arg(0) as isize {
        SCHED_OTHER_RAW | SCHED_BATCH_RAW | SCHED_IDLE_RAW | SCHED_DEADLINE_RAW => 0,
        SCHED_FIFO_RAW | SCHED_RR_RAW => 99,
        _ => return UserRet::from_error(ErrNo::EINVAL),
    };
    UserRet::from_success(max as isize as usize)
}

/// `sched_get_priority_min(policy)`。
// 本方法代码由AI完成
pub(crate) fn sys_sched_get_priority_min(args : SyscallArgs) -> UserRet {
    let min = match args.arg(0) as isize {
        SCHED_OTHER_RAW | SCHED_BATCH_RAW | SCHED_IDLE_RAW | SCHED_DEADLINE_RAW => 0,
        SCHED_FIFO_RAW | SCHED_RR_RAW => 1,
        _ => return UserRet::from_error(ErrNo::EINVAL),
    };
    UserRet::from_success(min as isize as usize)
}
