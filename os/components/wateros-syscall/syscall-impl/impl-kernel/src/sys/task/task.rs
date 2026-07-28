//! 进程生命周期类系统调用：`yield`、`exit`、`exit_group`、`prctl`。
//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

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
const PR_CAPBSET_READ : usize = 23;
const PR_CAPBSET_DROP : usize = 24;
const PR_GET_CHILD_SUBREAPER : usize = 36;
const PR_SET_CHILD_SUBREAPER : usize = 37;

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
    if let Some(task_id) = task::current_task_id() {
        if let Some(process_task) = task::process_task_snapshot(task_id) {
            let last_thread = task::task_exit_would_finish_process(task_id)
                .unwrap_or(process_task.role == task::ProcessTaskRole::Leader);
            if last_thread {
                super::super::acct::record_current_process_exit(exit_code);
                crate::sys::ipc::signal::notify_parent_sigchld(process_task.pid);
            }
            crate::sys::ipc::signal::on_thread_exit(task_id,
                                                    process_task.pid
                                                                .raw(),
                                                    last_thread);
        }
        super::wait::wake_clear_child_tid_for_task(task_id);
        crate::sys::ipc::robust::robust_exit_cleanup(task_id);
        super::wait::drop_task_runtime_resources(task_id);
    }
    crate::sys::misc::bringup_stats::record_sys_exit();
    task::exit_current(exit_code)
}

pub(crate) fn sys_exit_group(exit_code : isize) -> isize {
    let exit_code = normalize_user_exit_code(exit_code);
    if let Some(task_id) = task::current_task_id() {
        super::wait::wake_clear_child_tid_for_task(task_id);
        if let Some(process_task) = task::current_process_task_snapshot() {
            super::wait::reap_exited_member_threads_runtime_resources(process_task.pid);
            super::super::acct::record_current_process_exit(exit_code);
            crate::sys::ipc::signal::notify_parent_sigchld(process_task.pid);
            if let Some(task_ids) = task::task_ids_for_process(process_task.pid) {
                let user_aspace = task::current_task_user_aspace_ptr();
                for sibling in task_ids {
                    if sibling != task_id {
                        super::wait::wake_clear_child_tid_for_task(sibling);
                        crate::sys::ipc::robust::robust_exit_cleanup(sibling);
                        super::super::shm::drop_task_attachments(sibling, user_aspace);
                        super::wait::drop_task_runtime_resources(sibling);
                    }
                }
            }
            crate::sys::ipc::signal::on_thread_exit(task_id,
                                                    process_task.pid
                                                                .raw(),
                                                    true);
        }
        crate::sys::ipc::robust::robust_exit_cleanup(task_id);
        super::wait::drop_task_runtime_resources(task_id);
    }
    task::exit_group_current(exit_code)
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
            let dumpable = args.arg(1) != 0;
            if task::set_process_dumpable(current_pid, dumpable).is_ok() {
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
        PR_GET_CHILD_SUBREAPER => match task::process_child_subreaper(current_pid) {
            Some(true) => UserRet::from_success(1),
            Some(false) => UserRet::from_success(0),
            None => UserRet::from_error(ErrNo::ESRCH),
        },
        PR_SET_PDEATHSIG => {
            let sig = args.arg(1) as i32;
            if task::set_process_parent_death_signal(current_pid, sig).is_ok() {
                UserRet::from_success(0)
            } else {
                UserRet::from_error(ErrNo::ESRCH)
            }
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
        PR_SET_NO_NEW_PRIVS => UserRet::from_success(0),
        PR_CAPBSET_READ => super::super::cred::cap::cap_bset_read(args.arg(1)),
        PR_CAPBSET_DROP => super::super::cred::cap::cap_bset_drop(args.arg(1)),
        _ => UserRet::from_error(ErrNo::ENOSYS),
    }
}
