//! `fallocate(2)`：预分配已打开普通文件的区间（首期映射为按需 `truncate` 扩展）。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::vfs_util::vfs_io_at_error_to_errno;

const FALLOC_FL_KEEP_SIZE: u32 = 0x01;
const FALLOC_FL_PUNCH_HOLE: u32 = 0x02;

// 本方法代码由AI完成
pub(crate) fn sys_fallocate(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let mode = args.arg(1) as u32;
    let offset = args.arg(2) as u64;
    let len = args.arg(3) as u64;

    if len == 0 {
        return UserRet::from_success(0);
    }

    if mode & !(FALLOC_FL_KEEP_SIZE | FALLOC_FL_PUNCH_HOLE) != 0 {
        return UserRet::from_error(ErrNo::EOPNOTSUPP);
    }
    if mode & FALLOC_FL_PUNCH_HOLE != 0 {
        return UserRet::from_error(ErrNo::EOPNOTSUPP);
    }

    let end = match offset.checked_add(len) {
        Some(end) => end,
        None => return UserRet::from_error(ErrNo::EINVAL),
    };

    let result = vfs::fd::with_current_io(fd, |handle| -> Result<(), VfsError> {
        if mode & FALLOC_FL_KEEP_SIZE != 0 {
            let meta = handle.metadata()?;
            if meta.size >= end {
                return Ok(());
            }
            log::trace!(
                "[syscall] fallocate(nr=47) KEEP_SIZE prealloc stub (size {} -> {})",
                meta.size,
                end,
            );
            return Ok(());
        }

        let meta = handle.metadata()?;
        if meta.size < end {
            handle.truncate(end)?;
        }
        Ok(())
    });

    match result {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Unsupported) => UserRet::from_error(ErrNo::EOPNOTSUPP),
        Err(e) => UserRet::from_error(vfs_io_at_error_to_errno(e)),
    }
}
