//! `flock(2)` — BSD 风格整文件 advisory 锁。
//! 本模块代码由AI完成

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

use crate::vfs_util::vfs_error_to_errno;
use vfs::fd;

// 本方法代码由AI完成
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

    let (key, owner) = vfs::fd::with_current_io(fd, |handle| {
        let meta = handle.metadata()?;
        let key = fd::inode_key_from_metadata(&meta).ok_or(vfs::api::VfsError::Unsupported)?;
        let owner = handle.flock_owner_id().ok_or(vfs::api::VfsError::Unsupported)?;
        Ok((key, owner))
    })
    .map_err(|err| match err {
        vfs::api::VfsError::Unsupported => ErrNo::EINVAL,
        other => vfs_error_to_errno(other),
    })?;

    fd::flock_op(&key, pid, owner, operation).map_err(vfs_error_to_errno)
}
