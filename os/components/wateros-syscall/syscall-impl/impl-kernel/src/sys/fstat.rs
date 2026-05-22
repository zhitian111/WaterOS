//! `fstat(2)`：将已打开文件的元数据写入用户 `stat` 缓冲。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::{VFS_FIRST_DYNAMIC_FD, VFS_STDERR_FD, VFS_STDIN_FD};

use crate::linux_stat::fill_linux_stat;
use crate::user_copy::copy_to_user_struct;
use crate::vfs_util::vfs_error_to_errno;

pub(crate) fn sys_fstat(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let stat_ptr = args.arg(1);
    if stat_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if fd < VFS_FIRST_DYNAMIC_FD && (fd > VFS_STDERR_FD || fd < VFS_STDIN_FD) {
        return UserRet::from_error(ErrNo::EBADF);
    }

    match vfs::fd::with_current_io(fd, |handle| {
              let meta = handle.metadata()?;
              Ok(fill_linux_stat(&meta, meta.size))
          }) {
        Ok(stat) => match copy_to_user_struct(stat_ptr, &stat) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        },
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}
