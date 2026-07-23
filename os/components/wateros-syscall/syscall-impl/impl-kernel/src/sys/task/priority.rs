//! `setpriority(2)` / `getpriority(2)`：维护 task-level nice 属性。
//! 本模块代码由AI完成
use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::current_credentials;
use task::{ProcessId, TaskId};

const PRIO_PROCESS : i32 = 0;
const PRIO_PGRP : i32 = 1;
const PRIO_USER : i32 = 2;
const NICE_MIN : i32 = -20;
const NICE_MAX : i32 = 19;

fn clamp_nice(prio : i32) -> i32 { prio.clamp(NICE_MIN, NICE_MAX) }

fn caller_is_privileged() -> bool {
    current_credentials().effective_uid
                         .0 ==
    0
}

fn resolve_task_target(who : i32) -> Result<TaskId, ErrNo> {
    if who < 0 {
        return Err(ErrNo::ESRCH);
    }
    if who == 0 {
        task::current_task_id().ok_or(ErrNo::ESRCH)
    } else {
        task::resolve_sched_pid(who as isize).map_err(|_| ErrNo::ESRCH)
    }
}

fn resolve_pgid_target(who : i32) -> Result<ProcessId, ErrNo> {
    if who < 0 {
        return Err(ErrNo::ESRCH);
    }
    let pgid = if who == 0 {
        let current = task::current_process_task_snapshot().ok_or(ErrNo::ESRCH)?;
        task::process_pgid(current.pid).ok_or(ErrNo::ESRCH)?
    } else {
        ProcessId::from_raw(who as usize)
    };
    if !task::pgid_has_members(pgid) {
        return Err(ErrNo::ESRCH);
    }
    Ok(pgid)
}

fn resolve_user_target(who : i32) -> Result<u32, ErrNo> {
    if who < 0 {
        return Err(ErrNo::ESRCH);
    }
    if who == 0 {
        return Ok(current_credentials().real_uid
                                       .0);
    }
    Ok(who as u32)
}

fn task_real_uid(task_id : TaskId) -> u32 {
    cred::credentials_for(task_id).real_uid
                                      .0
}

fn all_live_task_ids() -> Vec<TaskId> {
    let mut task_ids = Vec::new();
    for pid in task::all_process_pids() {
        if let Some(ids) = task::task_ids_for_process(pid) {
            for task_id in ids {
                if task::task_snapshot(task_id).is_some_and(|snapshot| {
                    !matches!(snapshot.state, task::TaskState::Exited(_))
                }) {
                    task_ids.push(task_id);
                }
            }
        }
    }
    task_ids
}

fn task_ids_in_pgid(pgid : ProcessId) -> Vec<TaskId> {
    let mut task_ids = Vec::new();
    for pid in task::all_process_pids() {
        if task::process_pgid(pid) == Some(pgid) {
            for task_id in task::task_ids_for_process(pid).unwrap_or_default() {
                if task::task_snapshot(task_id).is_some_and(|snapshot| {
                    !matches!(snapshot.state, task::TaskState::Exited(_))
                }) {
                    task_ids.push(task_id);
                }
            }
        }
    }
    task_ids
}

fn set_nice_for_tasks(task_ids : impl IntoIterator<Item = TaskId>, nice : i8) -> bool {
    let mut found = false;
    for task_id in task_ids {
        if task::set_nice(task_id, nice).is_ok() {
            found = true;
        }
    }
    found
}

fn min_nice_for_tasks(task_ids : impl IntoIterator<Item = TaskId>) -> Option<i32> {
    task_ids.into_iter()
            .filter_map(|task_id| task::get_nice(task_id).ok())
            .map(i32::from)
            .min()
}

fn set_nice_for_uid(uid : u32, nice : i8) -> bool {
    set_nice_for_tasks(all_live_task_ids().into_iter()
                                         .filter(|task_id| task_real_uid(*task_id) == uid),
                       nice)
}

fn min_nice_for_uid(uid : u32) -> Option<i32> {
    min_nice_for_tasks(all_live_task_ids().into_iter()
                                          .filter(|task_id| task_real_uid(*task_id) == uid))
}

fn check_setpermission(which : i32, who : i32, prio : i32) -> Result<(), ErrNo> {
    if caller_is_privileged() {
        return Ok(());
    }
    match which {
        PRIO_PROCESS => {
            // Linux 的 nice 是线程属性；`who == 0` 指当前线程。
            let target_uid = task_real_uid(resolve_task_target(who)?);
            let cred = current_credentials();
            if target_uid != cred.real_uid.0 && target_uid != cred.effective_uid.0 {
                return Err(ErrNo::EPERM);
            }
        }
        PRIO_PGRP => {
            // 仅验证 PGID 存在；实际按 task UID 的过滤在 sys_setpriority 中完成。
            resolve_pgid_target(who)?;
        }
        PRIO_USER => {
            let uid = resolve_user_target(who)?;
            let cred = current_credentials();
            if uid != cred.real_uid.0 && uid != cred.effective_uid.0 {
                return Err(ErrNo::EPERM);
            }
        }
        _ => return Err(ErrNo::EINVAL),
    }
    // 非特权用户不能降低 nice 值（提高优先级），等效于默认 RLIMIT_NICE=0。
    if prio < 0 {
        return Err(ErrNo::EACCES);
    }
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn sys_setpriority(args : SyscallArgs) -> UserRet {
    let which = args.arg(0) as i32;
    let who = args.arg(1) as i32;
    let prio = clamp_nice(args.arg(2) as i32);

    if !matches!(which,
                 PRIO_PROCESS | PRIO_PGRP | PRIO_USER)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if let Err(errno) = check_setpermission(which, who, prio) {
        return UserRet::from_error(errno);
    }

    match which {
        PRIO_PROCESS => {
            let task_id = match resolve_task_target(who) {
                Ok(task_id) => task_id,
                Err(errno) => return UserRet::from_error(errno),
            };
            if task::set_nice(task_id, prio as i8).is_err() {
                return UserRet::from_error(ErrNo::ESRCH);
            }
        }
        PRIO_PGRP => {
            let pgid = match resolve_pgid_target(who) {
                Ok(pgid) => pgid,
                Err(errno) => return UserRet::from_error(errno),
            };
            if caller_is_privileged() {
                if !set_nice_for_tasks(task_ids_in_pgid(pgid), prio as i8) {
                    return UserRet::from_error(ErrNo::ESRCH);
                }
            } else {
                let cred = current_credentials();
                let mut found = false;
                for task_id in task_ids_in_pgid(pgid) {
                    let owner_uid = task_real_uid(task_id);
                    if (owner_uid == cred.real_uid.0 || owner_uid == cred.effective_uid.0) &&
                       task::set_nice(task_id, prio as i8).is_ok()
                    {
                        found = true;
                    }
                }
                if !found {
                    return UserRet::from_error(ErrNo::ESRCH);
                }
            }
        }
        PRIO_USER => {
            let uid = match resolve_user_target(who) {
                Ok(uid) => uid,
                Err(errno) => return UserRet::from_error(errno),
            };
            if !set_nice_for_uid(uid, prio as i8) {
                return UserRet::from_error(ErrNo::ESRCH);
            }
        }
        _ => return UserRet::from_error(ErrNo::EINVAL),
    }
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_getpriority(args : SyscallArgs) -> UserRet {
    let which = args.arg(0) as i32;
    let who = args.arg(1) as i32;

    if !matches!(which,
                 PRIO_PROCESS | PRIO_PGRP | PRIO_USER)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let nice = match which {
        PRIO_PROCESS => {
            let task_id = match resolve_task_target(who) {
                Ok(task_id) => task_id,
                Err(errno) => return UserRet::from_error(errno),
            };
            match task::get_nice(task_id) {
                Ok(nice) => i32::from(nice),
                Err(_) => return UserRet::from_error(ErrNo::ESRCH),
            }
        }
        PRIO_PGRP => {
            let pgid = match resolve_pgid_target(who) {
                Ok(pgid) => pgid,
                Err(errno) => return UserRet::from_error(errno),
            };
            match min_nice_for_tasks(task_ids_in_pgid(pgid)) {
                Some(nice) => nice,
                None => return UserRet::from_error(ErrNo::ESRCH),
            }
        }
        PRIO_USER => {
            let uid = match resolve_user_target(who) {
                Ok(uid) => uid,
                Err(errno) => return UserRet::from_error(errno),
            };
            match min_nice_for_uid(uid) {
                Some(nice) => nice,
                None => return UserRet::from_error(ErrNo::ESRCH),
            }
        }
        _ => return UserRet::from_error(ErrNo::EINVAL),
    };
    UserRet::from_success((20 - nice) as usize)
}
