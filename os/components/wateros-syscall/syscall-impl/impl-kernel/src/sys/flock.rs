//! `flock(2)` — BSD 风格整文件 advisory 锁。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::vfs_util::vfs_error_to_errno;
use vfs::fd;

pub(crate) fn sys_flock(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let operation = args.arg(1);

    match flock_impl(fd, operation) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

fn flock_impl(fd: usize, operation: usize) -> Result<(), ErrNo> {
    let pid = task::current_process_task_snapshot()
        .map(|snap| snap.pid)
        .ok_or(ErrNo::ESRCH)?;

    let key = vfs::fd::with_current_io(fd, |handle| {
        let meta = handle.metadata()?;
        fd::inode_key_from_metadata(&meta).ok_or(vfs::api::VfsError::Unsupported)
    })
    .map_err(|err| match err {
        vfs::api::VfsError::Unsupported => ErrNo::EINVAL,
        other => vfs_error_to_errno(other),
    })?;

    fd::flock_op(&key, pid, operation).map_err(vfs_error_to_errno)
}
