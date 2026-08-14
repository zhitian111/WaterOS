//! `ioprio_set(2)` / `ioprio_get(2)`：维护可继承的线程级 I/O 优先级。
//!
//! TCB 保存 Linux 原始编码，因此 fork/clone 自动继承、exec 保留。当前块设备
//! 请求仍是同步提交，尚没有按优先级重排的异步 elevator；该属性先用于 ABI
//! 兼容、诊断以及未来块调度器消费，不能错误地影响 CPU 调度优先级。

extern crate alloc;

use alloc::vec::Vec;

use api_v0::{ErrNo, SyscallArgs, UserRet};
use cred::current_credentials;
use task::{ProcessId, TaskId};

const IOPRIO_WHO_PROCESS : usize = 1;
const IOPRIO_WHO_PGRP : usize = 2;
const IOPRIO_WHO_USER : usize = 3;

const IOPRIO_CLASS_SHIFT : usize = 13;
const IOPRIO_CLASS_NONE : usize = 0;
const IOPRIO_CLASS_RT : usize = 1;
const IOPRIO_CLASS_BE : usize = 2;
const IOPRIO_CLASS_IDLE : usize = 3;
const IOPRIO_DATA_MASK : usize = (1 << IOPRIO_CLASS_SHIFT) - 1;

fn validate_priority(value : usize, privileged : bool) -> Result<u16, ErrNo> {
    if value > u16::MAX as usize {
        return Err(ErrNo::EINVAL);
    }
    let class = value >> IOPRIO_CLASS_SHIFT;
    let data = value & IOPRIO_DATA_MASK;
    match class {
        IOPRIO_CLASS_NONE if data == 0 => {}
        IOPRIO_CLASS_RT if data <= 7 && privileged => {}
        IOPRIO_CLASS_RT if data <= 7 => return Err(ErrNo::EPERM),
        IOPRIO_CLASS_BE if data <= 7 => {}
        IOPRIO_CLASS_IDLE if data == 0 => {}
        _ => return Err(ErrNo::EINVAL),
    }
    Ok(value as u16)
}

fn live_task_ids() -> Vec<TaskId> {
    let mut ids = Vec::new();
    for pid in task::all_process_pids() {
        for task_id in task::task_ids_for_process(pid).unwrap_or_default() {
            if task::task_snapshot(task_id).is_some_and(|snapshot| {
                !matches!(snapshot.state, task::TaskState::Exited(_))
            }) {
                ids.push(task_id);
            }
        }
    }
    ids
}

fn resolve_process(who : usize) -> Result<TaskId, ErrNo> {
    if who == 0 {
        task::current_task_id().ok_or(ErrNo::ESRCH)
    } else {
        task::resolve_sched_pid(who as isize).map_err(|_| ErrNo::ESRCH)
    }
}

fn resolve_pgid(who : usize) -> Result<ProcessId, ErrNo> {
    let pgid = if who == 0 {
        let current = task::current_process_task_snapshot().ok_or(ErrNo::ESRCH)?;
        task::process_pgid(current.pid).ok_or(ErrNo::ESRCH)?
    } else {
        ProcessId::from_raw(who)
    };
    if task::pgid_has_members(pgid) {
        Ok(pgid)
    } else {
        Err(ErrNo::ESRCH)
    }
}

fn target_uid(who : usize) -> u32 {
    if who == 0 {
        current_credentials().real_uid.0
    } else {
        who as u32
    }
}

fn task_uid(task_id : TaskId) -> u32 { cred::credentials_for(task_id).real_uid.0 }

fn select_targets(which : usize, who : usize) -> Result<Vec<TaskId>, ErrNo> {
    match which {
        IOPRIO_WHO_PROCESS => Ok(alloc::vec![resolve_process(who)?]),
        IOPRIO_WHO_PGRP => {
            let pgid = resolve_pgid(who)?;
            let ids : Vec<_> = live_task_ids()
                .into_iter()
                .filter(|task_id| {
                    task::process_task_snapshot(*task_id)
                        .and_then(|snapshot| task::process_pgid(snapshot.pid)) == Some(pgid)
                })
                .collect();
            if ids.is_empty() { Err(ErrNo::ESRCH) } else { Ok(ids) }
        }
        IOPRIO_WHO_USER => {
            let uid = target_uid(who);
            let ids : Vec<_> = live_task_ids()
                .into_iter()
                .filter(|task_id| task_uid(*task_id) == uid)
                .collect();
            if ids.is_empty() { Err(ErrNo::ESRCH) } else { Ok(ids) }
        }
        _ => Err(ErrNo::EINVAL),
    }
}

fn can_modify(task_id : TaskId) -> bool {
    let caller = current_credentials();
    caller.effective_uid.0 == 0 || {
        let owner = task_uid(task_id);
        owner == caller.real_uid.0 || owner == caller.effective_uid.0
    }
}

pub(crate) fn sys_ioprio_set(args : SyscallArgs) -> UserRet {
    let which = args.arg(0);
    let who = args.arg(1);
    let privileged = current_credentials().effective_uid.0 == 0;
    let priority = match validate_priority(args.arg(2), privileged) {
        Ok(priority) => priority,
        Err(error) => return UserRet::from_error(error),
    };
    let targets = match select_targets(which, who) {
        Ok(targets) => targets,
        Err(error) => return UserRet::from_error(error),
    };
    if targets.iter().copied().any(|task_id| !can_modify(task_id)) {
        return UserRet::from_error(ErrNo::EPERM);
    }
    for task_id in targets {
        if task::set_io_priority(task_id, priority).is_err() {
            return UserRet::from_error(ErrNo::ESRCH);
        }
    }
    UserRet::from_success(0)
}

fn effective_order(value : u16) -> (u8, u16) {
    let value = value as usize;
    let class = value >> IOPRIO_CLASS_SHIFT;
    let data = (value & IOPRIO_DATA_MASK) as u16;
    match class {
        IOPRIO_CLASS_RT => (0, data),
        IOPRIO_CLASS_BE => (1, data),
        // NONE 由 Linux 视作由 nice 推导的 best-effort；尚未指定时用中档排序。
        IOPRIO_CLASS_NONE => (1, 4),
        IOPRIO_CLASS_IDLE => (2, 0),
        _ => (3, data),
    }
}

pub(crate) fn sys_ioprio_get(args : SyscallArgs) -> UserRet {
    let targets = match select_targets(args.arg(0), args.arg(1)) {
        Ok(targets) => targets,
        Err(error) => return UserRet::from_error(error),
    };
    let priority = targets.into_iter()
                          .filter_map(|task_id| task::get_io_priority(task_id).ok())
                          .min_by_key(|value| effective_order(*value));
    match priority {
        Some(priority) => UserRet::from_success(priority as usize),
        None => UserRet::from_error(ErrNo::ESRCH),
    }
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    assert_eq!(validate_priority(0, false), Ok(0));
    assert_eq!(validate_priority((IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | 7, false),
               Ok(((IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | 7) as u16));
    assert_eq!(validate_priority(IOPRIO_CLASS_RT << IOPRIO_CLASS_SHIFT, false),
               Err(ErrNo::EPERM));
    assert_eq!(validate_priority((IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT) | 1, true),
               Err(ErrNo::EINVAL));
}
