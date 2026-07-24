//! 进程标识类系统调用：`getpid`、`getppid`、`gettid`、`setsid`、`setpgid`、`getpgid`、`set_tid_address`。
//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use task::{ProcessId, TaskClearTid};

const ORPHAN_PARENT_PID : usize = 1;

pub(crate) fn sys_getpid() -> UserRet {
    task::current_process_task_snapshot().map(|snapshot| UserRet::from_success(snapshot.pid.raw()))
                                         .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

pub(crate) fn sys_getppid() -> UserRet {
    let snapshot = match task::current_process_snapshot() {
        Some(snapshot) => snapshot,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    UserRet::from_success(snapshot.parent_pid
                                  .map(|pid| pid.raw())
                                  .unwrap_or(ORPHAN_PARENT_PID))
}

pub(crate) fn sys_gettid() -> UserRet {
    task::current_thread_id().map(|tid| UserRet::from_success(tid.raw()))
                             .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

pub(crate) fn sys_setsid() -> UserRet {
    let current = match task::current_process_snapshot() {
        Some(process) => process,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    if current.pgid == current.pid {
        return UserRet::from_error(ErrNo::EPERM);
    }
    match task::create_session_for_process(current.pid) {
        Ok(()) => UserRet::from_success(current.pid.raw()),
        Err(task::ProcessError::AlreadySessionLeader) => UserRet::from_error(ErrNo::EPERM),
        Err(task::ProcessError::ProcessNotFound) => UserRet::from_error(ErrNo::ESRCH),
        _ => UserRet::from_error(ErrNo::EPERM),
    }
}

/// 检查 `pgid` 是否是对应会话 `sid` 中的已有进程组。
fn pgid_exists_in_session(pgid : ProcessId, sid : ProcessId) -> bool {
    task::all_process_pids().into_iter()
                            .any(|pid| {
                                task::process_snapshot(pid).is_some_and(|s| {
                                                               s.pgid == pgid && s.sid == sid
                                                           })
                            })
}

/// `setpgid(2)`：维护 pgid 并在常见自调用/父子场景下返回成功。
///
/// 规则：
/// - 目标必须是自身或子进程（子进程需在 Running/Stopped 状态）
/// - 会话首进程不能修改 PGID
/// - 加入已有进程组时，该组必须存在于同一会话中
pub(crate) fn sys_setpgid(args : SyscallArgs) -> UserRet {
    let pid_arg = args.arg(0) as i32;
    let pgid_arg = args.arg(1) as i32;

    if pgid_arg < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if pid_arg < 0 {
        return UserRet::from_error(ErrNo::ESRCH);
    }

    let current = match task::current_process_snapshot() {
        Some(process) => process,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };

    let target_pid = if pid_arg == 0 {
        current.pid
    } else {
        ProcessId::from_raw(pid_arg as usize)
    };
    let new_pgid = if pgid_arg == 0 {
        target_pid
    } else {
        ProcessId::from_raw(pgid_arg as usize)
    };

    let target_snapshot = match task::process_snapshot(target_pid) {
        Some(snapshot) => snapshot,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };

    // Linux: 目标必须是自身或子进程
    if target_pid != current.pid && target_snapshot.parent_pid != Some(current.pid) {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    // Linux: 子进程必须在 Running 或 Stopped 状态
    if target_pid != current.pid &&
       target_snapshot.parent_pid == Some(current.pid) &&
       !matches!(target_snapshot.state,
                 task::ProcessState::Running | task::ProcessState::Stopped { .. })
    {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    // Linux: 会话首进程不能修改 PGID
    if target_snapshot.sid
                      .raw() !=
       0 &&
       target_snapshot.sid == target_pid
    {
        return UserRet::from_error(ErrNo::EPERM);
    }
    // Linux: 加入已有进程组时，该组必须存在于同一会话中
    if new_pgid != target_pid {
        if target_snapshot.sid
                          .raw() ==
           0
        {
            return UserRet::from_error(ErrNo::EPERM);
        }
        if !pgid_exists_in_session(new_pgid, target_snapshot.sid) {
            return UserRet::from_error(ErrNo::EPERM);
        }
    }
    if task::set_process_pgid(target_pid, new_pgid).is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    UserRet::from_success(0)
}

/// `getpgid(2)`：返回进程所属进程组 id。
pub(crate) fn sys_getpgid(args : SyscallArgs) -> UserRet {
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

pub(crate) fn sys_set_tid_address(args : SyscallArgs) -> UserRet {
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
