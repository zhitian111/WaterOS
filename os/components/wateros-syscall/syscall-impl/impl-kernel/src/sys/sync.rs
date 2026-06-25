//! `sync(2)` / `fsync(2)` / `fdatasync(2)`：将脏数据刷回后端。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::vfs_util::vfs_error_to_errno;

fn sync_fd(op : &str, fd : usize) -> UserRet {
    match vfs::fd::with_current_io(fd, |handle| handle.flush()) {
        Ok(()) => UserRet::from_success(0),
        Err(err) => {
            let errno = vfs_error_to_errno(err);
            if matches!(errno, ErrNo::EIO | ErrNo::EINVAL | ErrNo::EAGAIN) {
                log::warn!("[syscall] {} fd={} flush failed: {:?}", op, fd, err);
            }
            UserRet::from_error(errno)
        }
    }
}

pub(crate) fn sys_fsync(args : SyscallArgs) -> UserRet { sync_fd("fsync", args.arg(0)) }

pub(crate) fn sys_fdatasync(args : SyscallArgs) -> UserRet { sync_fd("fdatasync", args.arg(0)) }

pub(crate) fn sys_sync(_args : SyscallArgs) -> UserRet {
    // Linux sync(2) 发起系统级写回并始终返回 0；单个写回错误由后续文件操作报告。
    let _ = vfs::fd::flush_all_open_files();
    UserRet::from_success(0)
}
