//! `set_robust_list` / `get_robust_list` 与线程退出时的 robust futex 深清理。

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use ipc::futex::{
    FutexKey, RobustListHead, FUTEX_OWNER_DIED, FUTEX_TID_MASK, FUTEX_WAITERS,
    ROBUST_LIST_HEAD_SIZE, ROBUST_LIST_LIMIT,
};
use task::TaskId;

use crate::user_copy::{
    atomic_compare_exchange_user_u32_in_aspace, atomic_load_user_u32_in_aspace,
    copy_from_user_struct_in_aspace, copy_to_user_struct,
};

const FUTEX_ROBUST_MOD_MASK : usize = 1;

use super::futex_error_to_errno;

fn robust_entry(raw : usize) -> (usize, bool) {
    (raw & !FUTEX_ROBUST_MOD_MASK, raw & FUTEX_ROBUST_MOD_MASK != 0)
}

fn mark_owner_died(user_aspace : usize, entry : usize, futex_offset : isize, tid : usize) {
    let Some(futex_uaddr) = entry.checked_add_signed(futex_offset) else {
        return;
    };
    if futex_uaddr % core::mem::size_of::<u32>() != 0 {
        return;
    }
    loop {
        let Ok(old) = atomic_load_user_u32_in_aspace(user_aspace, futex_uaddr) else {
            return;
        };
        if (old & FUTEX_TID_MASK) as usize != tid {
            return;
        }
        let new = (old & FUTEX_WAITERS) | FUTEX_OWNER_DIED;
        let Ok(observed) =
            atomic_compare_exchange_user_u32_in_aspace(user_aspace, futex_uaddr, old, new)
        else {
            return;
        };
        if observed != old {
            continue;
        }
        if old & FUTEX_WAITERS != 0 {
            let private = FutexKey::private(futex_uaddr, user_aspace);
            if ipc::futex::wake(private, 1) == 0 {
                if let Ok(shared) =
                    super::futex::nonprivate_futex_key_for_aspace(user_aspace, futex_uaddr)
                {
                    let _ = ipc::futex::wake(shared, 1);
                }
            }
        }
        return;
    }
}

/// 线程退出前遍历用户 robust 链表并唤醒 waiters。
pub(crate) fn robust_exit_cleanup(task_id : TaskId) {
    let Some(registration) = ipc::futex::take_robust_list(task_id) else {
        return;
    };
    let tid = match task::process_task_snapshot(task_id) {
        Some(snapshot) => snapshot.tid.raw(),
        None => return,
    };
    let head_ptr = registration.head;
    let user_aspace = registration.user_aspace;
    if head_ptr == 0 {
        return;
    }

    let head = match copy_from_user_struct_in_aspace::<RobustListHead>(user_aspace, head_ptr) {
        Ok(h) => h,
        Err(_) => return,
    };

    let list_head = head_ptr;
    let futex_offset = head.futex_offset;
    let (pending, pending_is_pi) = robust_entry(head.list_op_pending);
    let (mut entry, mut entry_is_pi) = robust_entry(head.list);
    let mut steps = 0usize;

    while entry != list_head && entry != 0 && steps < ROBUST_LIST_LIMIT {
        steps += 1;
        let next_raw = match copy_from_user_struct_in_aspace::<usize>(user_aspace, entry) {
            Ok(next) => next,
            Err(_) => return,
        };
        if entry != pending && !entry_is_pi {
            mark_owner_died(user_aspace, entry, futex_offset, tid);
        }
        (entry, entry_is_pi) = robust_entry(next_raw);
    }
    if steps == ROBUST_LIST_LIMIT && entry != list_head && entry != 0 {
        log::warn!("[robust] list traversal limit reached task_id={} head={:#x}",
                   task_id,
                   head_ptr);
    }
    if pending != 0 && !pending_is_pi {
        // pending 可能已在主链表中，上面的 `entry != pending` 保证只处理一次。
        mark_owner_died(user_aspace, pending, futex_offset, tid);
    }
}

pub(crate) fn drop_robust_state(task_id : TaskId) { ipc::futex::drop_robust_list(task_id); }

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

pub(crate) fn sys_set_robust_list(args : SyscallArgs) -> UserRet {
    let head = args.arg(0);
    let len = args.arg(1);
    if len != ROBUST_LIST_HEAD_SIZE {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let tid = match task::current_task_id() {
        Some(tid) => tid,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    // 与 Linux ABI 一致：set 只登记指针并校验结构大小，不提前解引用用户链表。
    let user_aspace = task::current_task_user_aspace_ptr();
    match ipc::futex::set_robust_list(tid, head, len, user_aspace) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(futex_error_to_errno(e)),
    }
}

pub(crate) fn sys_get_robust_list(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let head_out = args.arg(1);
    let len_out = args.arg(2);
    if head_out == 0 || len_out == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let target_tid = match task::resolve_sched_pid(pid) {
        Ok(tid) => tid,
        Err(_) => return UserRet::from_error(ErrNo::ESRCH),
    };
    if let (Some(current), Some(target)) =
        (task::current_process_task_snapshot(), task::process_task_snapshot(target_tid))
    {
        if current.pid != target.pid {
            log::warn!("[syscall] get_robust_list(nr=100) pid={pid} not in current thread group",);
            return UserRet::from_error(ErrNo::EPERM);
        }
    }
    let (head, len) = ipc::futex::get_robust_list(target_tid);
    if copy_to_user_struct(len_out, &len).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if copy_to_user_struct(head_out, &head).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    UserRet::from_success(0)
}
