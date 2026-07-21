//! 进程标识类系统调用：`getpid`、`getppid`、`gettid`、`setsid`、`setpgid`、`getpgid`、`set_tid_address`。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use task::{ProcessId, TaskClearTid};

const ORPHAN_PARENT_PID: usize = 1;

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
    task::current_thread_id()
        .map(|tid| UserRet::from_success(tid.raw()))
        .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

pub(crate) fn sys_setsid() -> UserRet {
    let current = match task::current_process_snapshot() {
        Some(process) => process,
        None => return UserRet::from_error(ErrNo::EPERM),
    };
    if current.pgid == current.pid {
        return UserRet::from_error(ErrNo::EPERM);
    }
    match task::create_session_for_process(current.pid) {
        Ok(()) => UserRet::from_success(current.pid.raw()),
        Err(()) => UserRet::from_error(ErrNo::EPERM),
    }
}

/// `setpgid(2)`：维护 pgid 并在常见自调用/父子场景下返回成功。
pub(crate) fn sys_setpgid(args: SyscallArgs) -> UserRet {
    let pid_arg = args.arg(0) as i32;
    let pgid_arg = args.arg(1) as i32;

    if pgid_arg < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let current = match task::current_process_snapshot() {
        Some(process) => process,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let current_pid = i32::try_from(current.pid.raw()).unwrap_or(i32::MAX);

    let target_pid = if pid_arg == 0 {
        current_pid
    } else {
        pid_arg
    };
    let new_pgid_raw = if pgid_arg == 0 {
        usize::try_from(target_pid).unwrap_or(usize::MAX)
    } else {
        usize::try_from(pgid_arg).unwrap_or(usize::MAX)
    };
    let target = ProcessId::from_raw(usize::try_from(target_pid).unwrap_or(usize::MAX));
    let new_pgid = ProcessId::from_raw(new_pgid_raw);

    let target_snapshot = match task::process_snapshot(target) {
        Some(snapshot) => snapshot,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    if target != current.pid && target_snapshot.parent_pid != Some(current.pid) {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    if target != current.pid
        && target_snapshot.parent_pid == Some(current.pid)
        && !matches!(
            target_snapshot.state,
            task::ProcessState::Running | task::ProcessState::Stopped { .. }
        )
    {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    if target_snapshot.sid.raw() != 0 && target_snapshot.sid != target && new_pgid != target {
        return UserRet::from_error(ErrNo::EPERM);
    }
    if !task::set_process_pgid(target, new_pgid) {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    UserRet::from_success(0)
}

/// `getpgid(2)`：返回进程所属进程组 id。
pub(crate) fn sys_getpgid(args: SyscallArgs) -> UserRet {
    let pid_arg = args.arg(0) as i32;
    if pid_arg < 0 {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    let target_pid = if pid_arg == 0 {
        match task::current_process_task_snapshot() {
            Some(snapshot) => snapshot.pid,
            None => return UserRet::from_error(ErrNo::ESRCH),
        }
    } else {
        ProcessId::from_raw(pid_arg as usize)
    };
    if !task::process_exists(target_pid) {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    match task::process_pgid(target_pid) {
        Some(pgid) => UserRet::from_success(pgid.raw()),
        None => UserRet::from_error(ErrNo::ESRCH),
    }
}

pub(crate) fn sys_set_tid_address(args: SyscallArgs) -> UserRet {
    let task_id = match task::current_task_id() {
        Some(task_id) => task_id,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let tid = match task::current_thread_id() {
        Some(tid) => tid,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let user_addr = args.arg(0);
    let clear_child_tid = if user_addr == 0 {
        None
    } else {
        Some(TaskClearTid::new(user_addr))
    };
    let _ = task::set_task_clear_child_tid(task_id, clear_child_tid);
    UserRet::from_success(tid.raw())
}
