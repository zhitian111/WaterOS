//! `set_robust_list` / `get_robust_list` 与线程退出时的 robust futex 深清理。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::futex::{
    FutexError, FutexKey, FutexHub, KernelFutexOps, RobustListHead, FUTEX_OWNER_DIED, FUTEX_TID_MASK,
    ROBUST_LIST_HEAD_SIZE, ROBUST_LIST_LIMIT,
};
use task::TaskId;

use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user_struct};

const FUTEX_FLAG_MASK: u32 = !FUTEX_TID_MASK;

pub(crate) fn futex_error_to_errno(error: FutexError) -> ErrNo {
    match error {
        FutexError::Again => ErrNo::EAGAIN,
        FutexError::Fault => ErrNo::EFAULT,
        FutexError::Invalid => ErrNo::EINVAL,
        FutexError::Nosys => ErrNo::ENOSYS,
        FutexError::TimedOut => ErrNo::ETIMEDOUT,
        FutexError::Interrupted => ErrNo::EINTR,
    }
}

fn read_user_u32(uaddr: usize) -> Result<u32, ErrNo> {
    let mut val: u32 = 0;
    let buf = unsafe { core::slice::from_raw_parts_mut((&raw mut val) as *mut u8, 4) };
    if copy_from_user(buf, uaddr)? != 4 {
        return Err(ErrNo::EFAULT);
    }
    Ok(val)
}

fn read_user_list_next(entry: usize) -> Result<usize, ErrNo> {
    let mut next: usize = 0;
    let buf = unsafe { core::slice::from_raw_parts_mut((&raw mut next) as *mut u8, 8) };
    if copy_from_user(buf, entry)? != 8 {
        return Err(ErrNo::EFAULT);
    }
    Ok(next)
}

/// 线程退出前遍历用户 robust 链表并唤醒 waiters。
pub(crate) fn robust_exit_cleanup(task_id: TaskId) {
    let hub = FutexHub::global();
    let tid = match task::process_task_snapshot(task_id) {
        Some(snapshot) => snapshot.tid.raw(),
        None => {
            hub.drop_robust_list(task_id);
            return;
        }
    };
    let (head_ptr, _len) = match hub.get_robust_list(task_id) {
        Ok(state) => state,
        Err(_) => return,
    };
    if head_ptr == 0 {
        hub.drop_robust_list(task_id);
        return;
    }

    let head = match copy_from_user_struct::<RobustListHead>(head_ptr) {
        Ok(h) => h,
        Err(_) => {
            hub.drop_robust_list(task_id);
            return;
        }
    };

    if head.list_op_pending != 0 {
        // 首版跳过 list_op；BusyBox 常规路径多为 0。
    }

    let list_head = head_ptr;
    let futex_offset = head.futex_offset;
    let mut entry = head.list;
    let mut steps = 0usize;

    while entry != list_head && entry != 0 && steps < ROBUST_LIST_LIMIT {
        steps += 1;
        let futex_uaddr = entry.wrapping_add_signed(futex_offset);
        if let Ok(val) = read_user_u32(futex_uaddr) {
            let owner = val & FUTEX_TID_MASK;
            if owner as usize == tid {
                let new_val = (val & FUTEX_FLAG_MASK) | FUTEX_OWNER_DIED;
                let _ = copy_to_user_struct(futex_uaddr, &new_val);
                let key = FutexKey {
                    uaddr: futex_uaddr,
                    is_private: true,
                };
                let _ = hub.wake_all(key);
                let alt = FutexKey {
                    uaddr: futex_uaddr,
                    is_private: false,
                };
                let _ = hub.wake_all(alt);
            }
        }
        entry = match read_user_list_next(entry) {
            Ok(next) => next,
            Err(_) => break,
        };
    }

    hub.drop_robust_list(task_id);
}

pub(crate) fn drop_robust_state(task_id: TaskId) {
    FutexHub::global().drop_robust_list(task_id);
}

/// execve 前清理同进程其它线程的 robust 状态。
pub(crate) fn robust_exit_cleanup_siblings_for_exec() {
    let current_id = match task::current_task_id() {
        Some(id) => id,
        None => return,
    };
    let process_task = match task::current_process_task_snapshot() {
        Some(p) => p,
        None => return,
    };
    let task_ids = match task::task_ids_for_process(process_task.pid) {
        Some(ids) => ids,
        None => return,
    };
    for task_id in task_ids {
        if task_id != current_id {
            robust_exit_cleanup(task_id);
        }
    }
}

pub(crate) fn sys_set_robust_list(args: SyscallArgs) -> UserRet {
    let head = args.arg(0);
    let len = args.arg(1);
    if len != ROBUST_LIST_HEAD_SIZE {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let tid = match task::current_task_id() {
        Some(tid) => tid,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    if head != 0 {
        if copy_from_user_struct::<RobustListHead>(head).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }
    match FutexHub::global().set_robust_list(tid, head, len) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(futex_error_to_errno(e)),
    }
}

pub(crate) fn sys_get_robust_list(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let head_out = args.arg(1);
    let len_out = args.arg(2);

    let target_tid = match task::resolve_sched_pid(pid) {
        Ok(tid) => tid,
        Err(_) => return UserRet::from_error(ErrNo::ESRCH),
    };
    if let (Some(current), Some(target)) = (
        task::current_process_task_snapshot(),
        task::process_task_snapshot(target_tid),
    ) {
        if current.pid != target.pid {
            log::warn!(
                "[syscall] get_robust_list(nr=100) pid={pid} not in current thread group",
            );
            return UserRet::from_error(ErrNo::EPERM);
        }
    }
    let (head, len) = match FutexHub::global().get_robust_list(target_tid) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(futex_error_to_errno(e)),
    };
    if head_out != 0 && copy_to_user_struct(head_out, &head).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if len_out != 0 && copy_to_user_struct(len_out, &len).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    UserRet::from_success(0)
}
