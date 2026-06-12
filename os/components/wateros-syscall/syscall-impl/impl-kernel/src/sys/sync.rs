//! `sync(2)` / `fsync(2)` / `fdatasync(2)`：存储同步系统调用。

use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::vfs_util::vfs_error_to_errno;

/// 当前文件写路径直接提交到后端，尚无全局页缓存/挂载表可枚举。
///
/// 因此 `sync(2)` 作为全局同步屏障成功返回；单 fd 的显式刷新仍由
/// `fsync(2)` / `fdatasync(2)` 调用 handle 的 `flush()` 完成。
pub(crate) fn sys_sync() -> UserRet {
    UserRet::from_success(0)
}

fn sync_fd(fd: usize) -> UserRet {
    match vfs::fd::with_current_io(fd, |handle| handle.flush()) {
        Ok(()) => UserRet::from_success(0),
        Err(err) => UserRet::from_error(vfs_error_to_errno(err)),
    }
}

pub(crate) fn sys_fsync(args: SyscallArgs) -> UserRet {
    sync_fd(args.arg(0))
}

pub(crate) fn sys_fdatasync(args: SyscallArgs) -> UserRet {
    sync_fd(args.arg(0))
}
