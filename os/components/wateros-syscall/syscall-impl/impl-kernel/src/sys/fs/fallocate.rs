//! `fallocate(2)`：预分配已打开普通文件的区间（首期映射为按需 `truncate` 扩展）。
//! 本模块代码由AI完成

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use vfs::api::VfsError;

use crate::vfs_util::vfs_io_at_error_to_errno;

const FALLOC_FL_KEEP_SIZE: u32 = 0x01;
const FALLOC_FL_PUNCH_HOLE: u32 = 0x02;

// 本方法代码由AI完成
pub(crate) fn sys_fallocate(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let mode = args.arg(1) as u32;
    let raw_offset = args.arg(2);
    let raw_len = args.arg(3);

    if (raw_offset as isize) < 0 || (raw_len as isize) < 0 || raw_len == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let offset = raw_offset as u64;
    let len = raw_len as u64;

    if mode & !(FALLOC_FL_KEEP_SIZE | FALLOC_FL_PUNCH_HOLE) != 0 {
        return UserRet::from_error(ErrNo::EOPNOTSUPP);
    }
    if mode & FALLOC_FL_PUNCH_HOLE != 0 {
        return UserRet::from_error(ErrNo::EOPNOTSUPP);
    }

    let end = match offset.checked_add(len) {
        Some(end) if end <= i64::MAX as u64 => end,
        Some(_) => return UserRet::from_error(ErrNo::EFBIG),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };

    let result = loop {
        let result = vfs::fd::with_current_io(fd, |handle| -> Result<(), VfsError> {
            const O_ACCMODE: u32 = 3;
            const O_RDONLY: u32 = 0;
            if handle.open_accmode() & O_ACCMODE == O_RDONLY {
                return Err(VfsError::BadFd);
            }

            if mode & FALLOC_FL_KEEP_SIZE != 0 {
                let meta = handle.metadata()?;
                if meta.size >= end {
                    return Ok(());
                }
                // 当前 VFS 只能通过 truncate 改变可见文件长度，无法在保持 i_size
                // 不变时预留块。不能让调用者误以为空间已经得到保证。
                return Err(VfsError::Unsupported);
            }

            let meta = handle.metadata()?;
            if meta.size < end {
                handle.truncate(end)?;
            }
            Ok(())
        });
        if result != Err(VfsError::Busy) {
            break result;
        }
        task::yield_now();
    };

    match result {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Unsupported) => UserRet::from_error(ErrNo::EOPNOTSUPP),
        Err(e) => UserRet::from_error(vfs_io_at_error_to_errno(e)),
    }
}
