//! `ftruncate(2)`：调整已打开普通文件长度。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::vfs_util::vfs_error_to_errno;

// 本方法代码由AI完成
pub(crate) fn sys_ftruncate(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let len = args.arg(1);

    if (len as isize) < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    match vfs::fd::with_current_io(fd, |handle| {
        const O_ACCMODE: u32 = 3;
        const O_RDONLY: u32 = 0;
        if handle.open_accmode() & O_ACCMODE == O_RDONLY {
            return Err(vfs::api::VfsError::Unsupported);
        }
        handle.truncate(len as u64)
    }) {
        Ok(()) => UserRet::from_success(0),
        Err(vfs::api::VfsError::Unsupported) => UserRet::from_error(ErrNo::EINVAL),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
