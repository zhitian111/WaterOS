//! `clone`/`fork` 系统调用实现。
//!
//! fork 时会为子进程创建**独立地址空间**（通过 `mm::kernel_mm::fork_user_aspace`），
//! 复制父进程 trap 帧（a0 置 0 作为子进程返回值），继承 cwd 与 fd 表（经 VFS duplicate）。
//!
//! clone（`child_stack ≠ 0`）时子进程使用调用者提供的独立栈。
//! fork（`child_stack == 0`）时子进程 SP 由 [`task`] 层 `fork_from` 按父栈区间设置。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::copy_to_user_struct;

/// clone/fork 系统调用入口。
///
/// 参数（Linux riscv64 clone ABI）：
/// - `arg0`: flags
/// - `arg1`: child_stack（0 表示复用父任务栈指针）
/// - `arg2`: parent_tid
/// - `arg3`: tls
/// - `arg4`: child_tid
pub(crate) fn sys_clone(args: SyscallArgs) -> UserRet {
    do_clone(args)
}

#[inline(never)]
fn do_clone(args: SyscallArgs) -> UserRet {
    let clone_flags = task::CloneFlags::from_bits(args.arg(0));
    let child_stack = args.arg(1);
    let parent_tid = args.arg(2);
    let tls = args.arg(3);
    let child_tid = args.arg(4);

    if clone_flags.contains(task::CloneFlags::CLONE_THREAD)
        && !clone_flags.contains(task::CloneFlags::CLONE_VM)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if clone_flags.contains(task::CloneFlags::CLONE_SIGHAND)
        && !clone_flags.contains(task::CloneFlags::CLONE_VM)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if clone_flags.contains(task::CloneFlags::CLONE_THREAD)
        && !clone_flags.contains(task::CloneFlags::CLONE_SIGHAND)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let is_thread = clone_flags.contains(task::CloneFlags::CLONE_VM)
        && clone_flags.contains(task::CloneFlags::CLONE_THREAD);
    if is_thread {
        return do_clone_thread(
            clone_flags,
            child_stack,
            parent_tid,
            tls,
            child_tid,
        );
    }

    let parent_aspace = task::current_task_user_aspace_ptr();
    let (new_aspace_ptr, new_satp) = match mm::kernel_mm::fork_user_aspace(parent_aspace) {
        Ok(p) => p,
        Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
    };

    let child_id = match task::fork_current(child_stack, new_aspace_ptr, new_satp) {
        Some(id) => id,
        None => return UserRet::from_error(ErrNo::EAGAIN),
    };
    let child_pid = match task::process_task_snapshot(child_id) {
        Some(snapshot) => snapshot.pid.raw(),
        None => return UserRet::from_error(ErrNo::ESRCH),
    };

    // 子任务继承父任务 cwd
    let parent_id = task::current_task_id().expect("current task must exist after fork");
    vfs::cwd::copy_cwd_from_parent(child_id, parent_id);

    vfs::fd::copy_fd_table_from_parent(child_id, parent_id);

    cred::fork_cred(parent_id, child_id);

    UserRet::from_success(child_pid)
}

fn do_clone_thread(
    clone_flags: task::CloneFlags,
    child_stack: usize,
    parent_tid: usize,
    tls: usize,
    child_tid: usize,
) -> UserRet {
    let clear_child_tid = if clone_flags.contains(task::CloneFlags::CLONE_CHILD_CLEARTID) {
        Some(task::TaskClearTid::new(child_tid))
    } else {
        None
    };
    let child_id = match task::clone_current_thread(
        child_stack,
        tls,
        clone_flags,
        clear_child_tid,
    ) {
        Some(id) => id,
        None => return UserRet::from_error(ErrNo::EAGAIN),
    };
    let child_tid_raw = match task::process_task_snapshot(child_id) {
        Some(snapshot) => snapshot.tid.raw(),
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let child_tid_value = child_tid_raw as u32;

    if clone_flags.contains(task::CloneFlags::CLONE_PARENT_SETTID)
        && parent_tid != 0
        && copy_to_user_struct(parent_tid, &child_tid_value).is_err()
    {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if clone_flags.contains(task::CloneFlags::CLONE_CHILD_SETTID)
        && child_tid != 0
        && copy_to_user_struct(child_tid, &child_tid_value).is_err()
    {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let parent_id = task::current_task_id().expect("current task must exist after clone");
    vfs::cwd::share_cwd_from_parent(child_id, parent_id);
    vfs::fd::share_fd_table_from_parent(child_id, parent_id);
    cred::share_cred(parent_id, child_id);

    UserRet::from_success(child_tid_raw)
}
