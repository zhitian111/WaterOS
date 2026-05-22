//! `clone`/`fork` 系统调用实现。
//!
//! 当前仅支持最小 fork 语义（`clone` 不带 `CLONE_VM`/`CLONE_THREAD` 等标志）：
//! 创建一个子任务，共享父任务地址空间与用户栈，子任务获得父任务 trap 帧副本
//! （a0 置 0），并继承父任务的 cwd。
//!
//! 父任务返回子任务 PID，子任务返回 0。

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
    let parent_id = match task::current_task_id() {
        Some(id) => id,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };

    let child_stack = args.arg(1);
    match task::fork_current(child_stack) {
        Some(child_id) => {
            // 继承父任务的 cwd
            #[cfg(feature = "fd-session")]
            vfs::cwd::copy_cwd_from_parent(child_id, parent_id);
            // 父任务返回子任务 id
            UserRet::from_success(child_id)
        }
        None => UserRet::from_error(ErrNo::EAGAIN),
    }
}
