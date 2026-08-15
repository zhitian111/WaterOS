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
const PR_SET_KEEPCAPS : usize = 8;
const PR_GET_KEEPCAPS : usize = 7;
const PR_GET_TID_ADDRESS : usize = 18;
const PR_SET_SECCOMP : usize = 22;
const PR_SET_NAME : usize = 15;
const PR_GET_NAME : usize = 16;
const PR_SET_NO_NEW_PRIVS : usize = 38;
const PR_GET_NO_NEW_PRIVS : usize = 39;
const PR_CAPBSET_READ : usize = 23;
const PR_CAPBSET_DROP : usize = 24;
const PR_SET_TIMING : usize = 14;
const PR_GET_SECUREBITS : usize = 27;
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
    slacks.insert(child, TimerSlack { default_ns:
                                          current_ns,
                                      current_ns });
}

pub(crate) fn drop_timer_slack(task_id : usize) {
    TIMER_SLACKS.lock()
                .remove(&task_id);
}

pub(crate) fn sys_yield() -> UserRet {
    task::yield_now();
    UserRet::from_success(0)
}

#[inline]
fn normalize_user_exit_code(exit_code : isize) -> isize { (exit_code as usize & 0xFF) as isize }

pub(crate) fn sys_exit(exit_code : isize) -> isize {
    let exit_code = normalize_user_exit_code(exit_code);
    exit_current_with_wait_code(exit_code)
}

pub(crate) fn exit_current_with_wait_code(exit_code : isize) -> isize {
    let exiting_task_id = task::current_task_id();
    let mut process_task = None;
    let mut process_was_exiting = false;
    if let Some(task_id) = task::current_task_id() {
        if let Some(snapshot) = task::process_task_snapshot(task_id) {
            process_was_exiting = task::process_snapshot(snapshot.pid).is_some_and(|process| {
                                                                          matches!(process.state,
                                               task::ProcessState::Exiting(_))
                                                                      });
            process_task = Some(snapshot);
        }
        super::wait::wake_clear_child_tid_for_task(task_id);
        crate::sys::ipc::robust::robust_exit_cleanup(task_id);
        super::wait::drop_task_runtime_resources(task_id);
    }
    let exit_outcome = task::record_current_task_exit(exit_code);
    crate::sys::ipc::signal::deliver_parent_death_notifications(
        exit_outcome.parent_death_notifications.iter().copied());
    let completed_process = exit_outcome.completed_process;
    if let (Some(task_id), Some(process_task)) = (task::current_task_id(), process_task) {
        crate::sys::ipc::signal::on_thread_exit(task_id,
                                                process_task.pid
                                                            .raw(),
                                                completed_process.is_some());
    }
    if let Some(pid) = completed_process {
        tty::detach_session_by_sid(pid.raw());
        if tty::controlling_sid() == pid.raw() {
            tty::detach_controlling_terminal();
        }
        if !process_was_exiting {
            super::super::acct::record_current_process_exit(exit_code);
        }
        crate::sys::ipc::signal::notify_parent_sigchld(pid);
        task::wake_parent_child_waiters(pid);
    }
    crate::sys::misc::bringup_stats::record_sys_exit();
    super::vfork::complete_current();
    // 放在所有可能读取 current credentials 的退出收尾之后。下一步立即
    // 从 scheduler 退出当前任务，不再给用户态或普通 syscall 路径运行机会。
    if let Some(task_id) = exiting_task_id {
        cred::drop_task_cred(task_id);
    }
    task::exit_current(exit_code)
}

pub(crate) fn sys_exit_group(exit_code : isize) -> isize {
    let exit_code = normalize_user_exit_code(exit_code);
    exit_group_with_wait_code(exit_code)
}

pub(crate) fn exit_group_with_wait_code(exit_code : isize) -> isize {
    let exiting_task_id = task::current_task_id();
    let mut process_task = None;
    if let Some(task_id) = task::current_task_id() {
        super::wait::wake_clear_child_tid_for_task(task_id);
        if let Some(snapshot) = task::current_process_task_snapshot() {
            super::wait::reap_exited_member_threads_runtime_resources(snapshot.pid);
            super::super::acct::record_current_process_exit(exit_code);
            process_task = Some(snapshot);
            // Publish Exiting before any remote reschedule. Otherwise a sibling
            // can consume the IPI, still observe Running, and continue forever.
            let notifications = task::begin_current_process_exit(exit_code);
            crate::sys::ipc::signal::deliver_parent_death_notifications(notifications);
            if let Some(task_ids) = task::task_ids_for_process(snapshot.pid) {
                for sibling in task_ids {
                    if sibling != task_id {
                        // A blocked syscall may own stack-local pipe/socket leases.
                        // Marking it Exited remotely would skip Rust destructors and
                        // can keep a pipe writer alive after the process is already a
                        // zombie. Interrupt the wait instead; after its syscall stack
                        // unwinds, the trap-return ProcessState::Exiting check routes
                        // that thread through exit_current_with_wait_code and performs
                        // clear_child_tid, robust-list, fd and signal cleanup locally.
                        if !task::interrupt_task(sibling) {
                            task::request_task_reschedule(sibling);
                        }
                    }
                }
            }
        }
        crate::sys::ipc::robust::robust_exit_cleanup(task_id);
        super::wait::drop_task_runtime_resources(task_id);
    }
    let exit_outcome = task::record_current_task_exit(exit_code);
    crate::sys::ipc::signal::deliver_parent_death_notifications(
        exit_outcome.parent_death_notifications.iter().copied());
    let completed_process = exit_outcome.completed_process;
    if let (Some(task_id), Some(process_task)) = (task::current_task_id(), process_task) {
        crate::sys::ipc::signal::on_thread_exit(task_id,
                                                process_task.pid
                                                            .raw(),
                                                completed_process.is_some());
    }
    if let Some(pid) = completed_process {
        tty::detach_session_by_sid(pid.raw());
        if tty::controlling_sid() == pid.raw() {
            tty::detach_controlling_terminal();
        }
        crate::sys::ipc::signal::notify_parent_sigchld(pid);
        task::wake_parent_child_waiters(pid);
    }
    super::vfork::complete_current();
    if let Some(task_id) = exiting_task_id {
        cred::drop_task_cred(task_id);
    }
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
        PR_SET_KEEPCAPS => {
            // Linux: arg2 只能是 0/1；setuid 从 0 降到非 0 时按此标志决定
            // 是否保留 permitted 集合（见 cred::apply_uid_triplet）。
            if args.arg(1) > 1 {
                UserRet::from_error(ErrNo::EINVAL)
            } else if task::set_process_keep_caps(current_pid, args.arg(1) != 0).is_ok() {
                UserRet::from_success(0)
            } else {
                UserRet::from_error(ErrNo::ESRCH)
            }
        }
        PR_GET_KEEPCAPS => match task::process_keep_caps(current_pid) {
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
        PR_GET_SECUREBITS => {
            // Linux 对 PR_GET_SECUREBITS 忽略 arg2..arg5；capsh 调用时只传
            // option 一个参数（其余寄存器为垃圾值），若校验会误报 EINVAL。
            // WaterOS 未实现 securebits 语义（KEEPCAPS 恒开），固定报 0。
            UserRet::from_success(0)
        }
        PR_SET_SECUREBITS => {
            if args.arg(2) | args.arg(3) | args.arg(4) != 0 {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            // Linux 需要 CAP_SETPCAP；WaterOS 不存储 securebits，root 接受即可
            // （setpriv --securebits 流程依赖此成功）。
            if cred::current_credentials().effective_uid
                                          .0 ==
               0
            {
                UserRet::from_success(0)
            } else {
                UserRet::from_error(ErrNo::EPERM)
            }
        }
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
