//! `unshare(2)`：创建新挂载命名空间（`CLONE_NEWNS`）。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

const CLONE_NEWNS: usize = 0x0002_0000;

pub(crate) fn sys_unshare(args: SyscallArgs) -> UserRet {
    let flags = args.arg(0);
    if flags == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags != CLONE_NEWNS {
        log::warn!("[syscall] unshare(nr=272) unsupported flags={:#x}", flags);
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match task::current_task_id() {
        Some(id) => id,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    vfs::mount_ns::unshare_mount_ns(task_id);
    UserRet::from_success(0)
}
