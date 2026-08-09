//! 进程生命周期类系统调用：`yield`、`exit`、`exit_group`、`prctl`。
//! 本模块代码由AI完成
use alloc::collections::BTreeMap;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use spin::Mutex;

use crate::user_copy::{copy_from_user, copy_to_user_struct};

// prctl 操作码
const PR_SET_PDEATHSIG : usize = 1;
const PR_GET_PDEATHSIG : usize = 2;
const PR_SET_DUMPABLE : usize = 4;
const PR_GET_DUMPABLE : usize = 3;
const PR_GET_TID_ADDRESS : usize = 18;
const PR_SET_SECCOMP : usize = 22;
const PR_SET_NAME : usize = 15;
const PR_GET_NAME : usize = 16;
const PR_SET_NO_NEW_PRIVS : usize = 38;
const PR_GET_NO_NEW_PRIVS : usize = 39;
const PR_CAPBSET_READ : usize = 23;
const PR_CAPBSET_DROP : usize = 24;
const PR_SET_TIMING : usize = 14;
const PR_SET_SECUREBITS : usize = 28;
const PR_SET_TIMERSLACK : usize = 29;
const PR_GET_TIMERSLACK : usize = 30;
const PR_SET_THP_DISABLE : usize = 41;
const PR_GET_THP_DISABLE : usize = 42;
const PR_CAP_AMBIENT : usize = 47;
const PR_GET_SPECULATION_CTRL : usize = 52;
const PR_SET_CHILD_SUBREAPER : usize = 36;
const PR_GET_CHILD_SUBREAPER : usize = 37;
const NSIG : i32 = 64;

const DEFAULT_TIMER_SLACK_NS : u64 = 50_000;

#[derive(Clone, Copy)]
struct TimerSlack {
    default_ns : u64,
    current_ns : u64,
}

static TIMER_SLACKS : Mutex<BTreeMap<usize, TimerSlack>> = Mutex::new(BTreeMap::new());

pub(crate) fn timer_slack_for_task(task_id : usize) -> u64 {
    TIMER_SLACKS.lock()
                .entry(task_id)
                .or_insert(TimerSlack { default_ns : DEFAULT_TIMER_SLACK_NS,
                                        current_ns : DEFAULT_TIMER_SLACK_NS })
                .current_ns
}

pub(crate) fn copy_timer_slack(parent : usize, child : usize) {
    let mut slacks = TIMER_SLACKS.lock();
    let current_ns = slacks.entry(parent)
                           .or_insert(TimerSlack { default_ns : DEFAULT_TIMER_SLACK_NS,
                                                   current_ns : DEFAULT_TIMER_SLACK_NS })
                           .current_ns;
    slacks.insert(child,
                  TimerSlack { default_ns : current_ns,
                               current_ns });
}

pub(crate) fn sys_yield() -> UserRet {
    task::yield_now();
    UserRet::from_success(0)
}

#[inline]
fn normalize_user_exit_code(exit_code : isize) -> isize {
    (exit_code as usize & 0xFF) as isize
}

pub(crate) fn sys_exit(exit_code : isize) -> isize {
    let exit_code = normalize_user_exit_code(exit_code);
    exit_current_with_wait_code(exit_code)
}

pub(crate) fn exit_current_with_wait_code(exit_code : isize) -> isize {
    let mut process_task = None;
    let mut process_was_exiting = false;
    if let Some(task_id) = task::current_task_id() {
        if let Some(snapshot) = task::process_task_snapshot(task_id) {
            process_was_exiting = task::process_snapshot(snapshot.pid)
                .is_some_and(|process| matches!(process.state, task::ProcessState::Exiting(_)));
            process_task = Some(snapshot);
        }
        super::wait::wake_clear_child_tid_for_task(task_id);
        crate::sys::ipc::robust::robust_exit_cleanup(task_id);
        super::wait::drop_task_runtime_resources(task_id);
    }
    let completed_process = task::record_current_task_exit(exit_code);
    if let Some(pid) = completed_process {
        crate::sys::ipc::signal::notify_parent_death_signals(pid);
    }
    if let (Some(task_id), Some(process_task)) = (task::current_task_id(), process_task) {
        crate::sys::ipc::signal::on_thread_exit(task_id,
                                                process_task.pid.raw(),
                                                completed_process.is_some());
    }
    if let Some(pid) = completed_process {
        if !process_was_exiting {
            super::super::acct::record_current_process_exit(exit_code);
        }
        crate::sys::ipc::signal::notify_parent_sigchld(pid);
        task::wake_parent_child_waiters(pid);
    }
    crate::sys::misc::bringup_stats::record_sys_exit();
    super::vfork::complete_current();
    task::exit_current(exit_code)
}

pub(crate) fn sys_exit_group(exit_code : isize) -> isize {
    let exit_code = normalize_user_exit_code(exit_code);
    exit_group_with_wait_code(exit_code)
}

pub(crate) fn exit_group_with_wait_code(exit_code : isize) -> isize {
    let mut process_task = None;
    if let Some(task_id) = task::current_task_id() {
        super::wait::wake_clear_child_tid_for_task(task_id);
        if let Some(snapshot) = task::current_process_task_snapshot() {
            crate::sys::ipc::signal::notify_parent_death_signals(snapshot.pid);
            super::wait::reap_exited_member_threads_runtime_resources(snapshot.pid);
            super::super::acct::record_current_process_exit(exit_code);
            process_task = Some(snapshot);
            // Publish Exiting before any remote reschedule. Otherwise a sibling
            // can consume the IPI, still observe Running, and continue forever.
            task::begin_current_process_exit(exit_code);
            if let Some(task_ids) = task::task_ids_for_process(snapshot.pid) {
                let user_aspace = task::current_task_user_aspace_ptr();
                for sibling in task_ids {
                    if sibling != task_id {
                        // 正在远端 CPU 执行的线程不能由本 CPU 提前释放 cred/fd/
                        // futex 等运行时资源。kill_task 成功表示它已经不再执行；
                        // 失败的远端线程会在下一次返回用户态时观察到进程 Exited，
                        // 再通过自己的 sys_exit 路径完成清理。
                        if task::kill_task(sibling, exit_code) {
                            super::wait::wake_clear_child_tid_for_task(sibling);
                            crate::sys::ipc::robust::robust_exit_cleanup(sibling);
                            super::super::shm::drop_task_attachments(sibling, user_aspace);
                            super::wait::drop_task_runtime_resources(sibling);
                        } else {
                            task::request_task_reschedule(sibling);
                        }
                    }
                }
            }
        }
        crate::sys::ipc::robust::robust_exit_cleanup(task_id);
        super::wait::drop_task_runtime_resources(task_id);
    }
    let completed_process = task::record_current_task_exit(exit_code);
    if let (Some(task_id), Some(process_task)) = (task::current_task_id(), process_task) {
        crate::sys::ipc::signal::on_thread_exit(task_id,
                                                process_task.pid.raw(),
                                                completed_process.is_some());
    }
    if let Some(pid) = completed_process {
        crate::sys::ipc::signal::notify_parent_sigchld(pid);
        task::wake_parent_child_waiters(pid);
    }
    super::vfork::complete_current();
    task::exit_current(exit_code)
}

#[cfg(test)]
mod tests {
    use super::normalize_user_exit_code;

    #[test]
    fn user_exit_status_is_limited_to_low_byte() {
        assert_eq!(normalize_user_exit_code(0), 0);
        assert_eq!(normalize_user_exit_code(256), 0);
        assert_eq!(normalize_user_exit_code(-1), 255);
        assert_eq!(normalize_user_exit_code(-129), 127);
    }
}

pub(crate) fn sys_prctl(args : SyscallArgs) -> UserRet {
    let option = args.arg(0);
    let current_pid = match task::current_process_task_snapshot() {
        Some(snapshot) => snapshot.pid,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    match option {
        PR_SET_NAME => {
            let name_ptr = args.arg(1);
            if name_ptr == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let mut comm = [0u8; 16];
            if copy_from_user(&mut comm, name_ptr).is_err() {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            // Linux: 保证 NUL 终止（最后字节强制为 0）
            comm[15] = 0;
            if let Some(task_id) = task::current_task_id() {
                let _ = task::set_thread_comm(task_id, comm);
            }
            UserRet::from_success(0)
        }
        PR_GET_NAME => {
            let name_ptr = args.arg(1);
            if name_ptr == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let comm = task::current_task_id().and_then(task::thread_comm)
                                              .unwrap_or([0u8; 16]);
            match copy_to_user_struct(name_ptr, &comm) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        PR_SET_DUMPABLE => {
            let dumpable = args.arg(1);
            if dumpable > 1 {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            if task::set_process_dumpable(current_pid, dumpable != 0).is_ok() {
                UserRet::from_success(0)
            } else {
                UserRet::from_error(ErrNo::ESRCH)
            }
        }
        PR_GET_DUMPABLE => match task::process_dumpable(current_pid) {
            Some(true) => UserRet::from_success(1),
            Some(false) => UserRet::from_success(0),
            None => UserRet::from_error(ErrNo::ESRCH),
        },
        PR_GET_TID_ADDRESS => {
            let addr_ptr = args.arg(1);
            if addr_ptr == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let tid_addr = task::current_task_id().and_then(task::task_clear_child_tid)
                                                  .map(|clear| clear.user_addr())
                                                  .unwrap_or(0);
            match copy_to_user_struct(addr_ptr, &tid_addr) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        PR_SET_CHILD_SUBREAPER => {
            let enabled = args.arg(1) != 0;
            if task::set_process_child_subreaper(current_pid, enabled).is_ok() {
                UserRet::from_success(0)
            } else {
                UserRet::from_error(ErrNo::ESRCH)
            }
        }
        PR_GET_CHILD_SUBREAPER => {
            let addr_ptr = args.arg(1);
            if addr_ptr == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let Some(enabled) = task::process_child_subreaper(current_pid) else {
                return UserRet::from_error(ErrNo::ESRCH);
            };
            match copy_to_user_struct(addr_ptr, &(enabled as i32)) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        PR_SET_PDEATHSIG => {
            let raw_sig = args.arg(1) as i32;
            if raw_sig < 0 || raw_sig > NSIG {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            if task::set_process_parent_death_signal(current_pid, raw_sig).is_ok() {
                UserRet::from_success(0)
            } else {
                UserRet::from_error(ErrNo::ESRCH)
            }
        }
        PR_SET_TIMING => {
            if args.arg(1) != 0 {
                UserRet::from_error(ErrNo::EINVAL)
            } else {
                UserRet::from_success(0)
            }
        }
        PR_SET_NO_NEW_PRIVS => {
            if args.arg(1) == 1 && args.arg(2) == 0 {
                UserRet::from_success(0)
            } else {
                UserRet::from_error(ErrNo::EINVAL)
            }
        }
        PR_GET_NO_NEW_PRIVS => {
            if args.arg(1) | args.arg(2) | args.arg(3) | args.arg(4) != 0 {
                UserRet::from_error(ErrNo::EINVAL)
            } else {
                UserRet::from_success(0)
            }
        }
        PR_SET_THP_DISABLE => {
            if args.arg(2) | args.arg(3) | args.arg(4) != 0 {
                UserRet::from_error(ErrNo::EINVAL)
            } else {
                UserRet::from_success(0)
            }
        }
        PR_GET_THP_DISABLE => {
            if args.arg(1) | args.arg(2) | args.arg(3) | args.arg(4) != 0 {
                UserRet::from_error(ErrNo::EINVAL)
            } else {
                UserRet::from_success(0)
            }
        }
        PR_CAP_AMBIENT | PR_GET_SPECULATION_CTRL => UserRet::from_error(ErrNo::EINVAL),
        PR_SET_SECUREBITS => UserRet::from_error(ErrNo::EPERM),
        PR_SET_TIMERSLACK => {
            let Some(task_id) = task::current_task_id() else {
                return UserRet::from_error(ErrNo::ESRCH);
            };
            let mut slacks = TIMER_SLACKS.lock();
            let slot = slacks.entry(task_id)
                             .or_insert(TimerSlack { default_ns : DEFAULT_TIMER_SLACK_NS,
                                                     current_ns : DEFAULT_TIMER_SLACK_NS });
            slot.current_ns = if args.arg(1) == 0 {
                slot.default_ns
            } else {
                args.arg(1) as u64
            };
            UserRet::from_success(0)
        }
        PR_GET_TIMERSLACK => {
            let Some(task_id) = task::current_task_id() else {
                return UserRet::from_error(ErrNo::ESRCH);
            };
            UserRet::from_success(timer_slack_for_task(task_id) as usize)
        }
        PR_GET_PDEATHSIG => {
            let addr_ptr = args.arg(1);
            if addr_ptr == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let sig = task::process_parent_death_signal(current_pid).unwrap_or(0);
            match copy_to_user_struct(addr_ptr, &sig) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        PR_SET_SECCOMP => UserRet::from_error(ErrNo::EINVAL),
        PR_CAPBSET_READ => super::super::cred::cap::cap_bset_read(args.arg(1)),
        PR_CAPBSET_DROP => super::super::cred::cap::cap_bset_drop(args.arg(1)),
        _ => UserRet::from_error(ErrNo::EINVAL),
    }
}
