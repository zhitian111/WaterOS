//! `setpriority(2)` / `getpriority(2)`：仅维护 per-process nice 变量，不参与调度。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::current_credentials;
use task::ProcessId;

const PRIO_PROCESS: i32 = 0;
const PRIO_PGRP: i32 = 1;
const PRIO_USER: i32 = 2;
const NICE_MIN: i32 = -20;
const NICE_MAX: i32 = 19;

fn clamp_nice(prio: i32) -> i32 {
    prio.clamp(NICE_MIN, NICE_MAX)
}

fn caller_is_privileged() -> bool {
    current_credentials().effective_uid.0 == 0
}

fn resolve_process_target(who: i32) -> Result<ProcessId, ErrNo> {
    if who < 0 {
        return Err(ErrNo::ESRCH);
    }
    let pid = if who == 0 {
        task::current_process_task_snapshot()
            .ok_or(ErrNo::ESRCH)?
            .pid
    } else {
        ProcessId::from_raw(who as usize)
    };
    if !task::process_exists(pid) {
        return Err(ErrNo::ESRCH);
    }
    Ok(pid)
}

fn resolve_pgid_target(who: i32) -> Result<ProcessId, ErrNo> {
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

fn check_setpermission(which: i32, who: i32, prio: i32) -> Result<(), ErrNo> {
    if caller_is_privileged() {
        return Ok(());
    }
    let current = task::current_process_task_snapshot().ok_or(ErrNo::ESRCH)?;
    match which {
        PRIO_PROCESS => {
            let target = resolve_process_target(who)?;
            if target != current.pid {
                return Err(ErrNo::EPERM);
            }
            if prio < 0 {
                return Err(ErrNo::EACCES);
            }
        }
        PRIO_PGRP => {
            let pgid = resolve_pgid_target(who)?;
            let current_pgid = task::process_pgid(current.pid).ok_or(ErrNo::ESRCH)?;
            if pgid != current_pgid {
                return Err(ErrNo::EPERM);
            }
            if prio < 0 {
                return Err(ErrNo::EACCES);
            }
        }
        PRIO_USER => return Err(ErrNo::EINVAL),
        _ => return Err(ErrNo::EINVAL),
    }
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn sys_setpriority(args: SyscallArgs) -> UserRet {
    let which = args.arg(0) as i32;
    let who = args.arg(1) as i32;
    let prio = clamp_nice(args.arg(2) as i32);

    if !matches!(which, PRIO_PROCESS | PRIO_PGRP | PRIO_USER) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if which == PRIO_USER {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if let Err(errno) = check_setpermission(which, who, prio) {
        return UserRet::from_error(errno);
    }

    match which {
        PRIO_PROCESS => {
            let pid = match resolve_process_target(who) {
                Ok(pid) => pid,
                Err(errno) => return UserRet::from_error(errno),
            };
            if !task::set_process_nice(pid, prio) {
                return UserRet::from_error(ErrNo::ESRCH);
            }
        }
        PRIO_PGRP => {
            let pgid = match resolve_pgid_target(who) {
                Ok(pgid) => pgid,
                Err(errno) => return UserRet::from_error(errno),
            };
            if !task::set_nice_for_pgid(pgid, prio) {
                return UserRet::from_error(ErrNo::ESRCH);
            }
        }
        _ => return UserRet::from_error(ErrNo::EINVAL),
    }
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_getpriority(args: SyscallArgs) -> UserRet {
    let which = args.arg(0) as i32;
    let who = args.arg(1) as i32;

    if !matches!(which, PRIO_PROCESS | PRIO_PGRP | PRIO_USER) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if which == PRIO_USER {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let nice = match which {
        PRIO_PROCESS => {
            let pid = match resolve_process_target(who) {
                Ok(pid) => pid,
                Err(errno) => return UserRet::from_error(errno),
            };
            match task::process_nice(pid) {
                Some(nice) => nice,
                None => return UserRet::from_error(ErrNo::ESRCH),
            }
        }
        PRIO_PGRP => {
            let pgid = match resolve_pgid_target(who) {
                Ok(pgid) => pgid,
                Err(errno) => return UserRet::from_error(errno),
            };
            match task::min_nice_in_pgid(pgid) {
                Some(nice) => nice,
                None => return UserRet::from_error(ErrNo::ESRCH),
            }
        }
        _ => return UserRet::from_error(ErrNo::EINVAL),
    };
    UserRet::from_success(nice as usize)
}
