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

/// clone/fork 系统调用入口。
///
/// 参数（Linux riscv64 clone ABI）：
/// - `arg0`: flags（当前仅忽略）
/// - `arg1`: child_stack（0 表示复用父任务栈指针）
/// - 其余参数暂未处理
pub(crate) fn sys_clone(args : SyscallArgs) -> UserRet {
    do_clone(args.arg(1))
}

#[inline(never)]
fn do_clone(child_stack : usize) -> UserRet {
    let parent_aspace = task::current_task_user_aspace_ptr();
    let (new_aspace_ptr, new_satp) = match mm::kernel_mm::fork_user_aspace(parent_aspace) {
        Ok(p) => p,
        Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
    };

    let child_id = match task::fork_current(child_stack, new_aspace_ptr, new_satp) {
        Some(id) => id,
        None => return UserRet::from_error(ErrNo::EAGAIN),
    };

    // 子任务继承父任务 cwd
    let parent_id = task::current_task_id()
        .expect("current task must exist after fork");
    vfs::cwd::copy_cwd_from_parent(child_id, parent_id);

    vfs::fd::copy_fd_table_from_parent(child_id, parent_id);

    cred::fork_cred(parent_id, child_id);

    UserRet::from_success(child_id)
}
