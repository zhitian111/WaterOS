//! `sync(2)` / `fsync(2)` / `fdatasync(2)`：将脏数据刷回后端。
//! 本模块代码由AI完成

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

use crate::vfs_util::vfs_error_to_errno;

// 本方法代码由AI完成
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

// 本方法代码由AI完成
pub(crate) fn sys_fsync(args : SyscallArgs) -> UserRet { sync_fd("fsync", args.arg(0)) }

// 本方法代码由AI完成
pub(crate) fn sys_fdatasync(args : SyscallArgs) -> UserRet { sync_fd("fdatasync", args.arg(0)) }

// 本方法代码由AI完成
pub(crate) fn sys_sync(_args : SyscallArgs) -> UserRet {
    // Linux sync(2) 发起系统级写回并始终返回 0；单个写回错误由后续文件操作报告。
    let _ = vfs::sync_file_page_cache();
    UserRet::from_success(0)
}

/// `syncfs(fd)` — 刷回 `fd` 所在文件系统的脏数据。
///
/// WaterOS 当前只有一个可写根卷和一个全局页缓存，因此“fd 所在文件系统”最终
/// 映射为全局页缓存写回。与 `sync()` 不同，`syncfs()` 必须检查 fd，并把写回
/// 错误返回给调用者。
pub(crate) fn sys_syncfs(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    if let Err(error) = vfs::fd::with_current_io(fd, |_handle| Ok(())) {
        return UserRet::from_error(vfs_error_to_errno(error));
    }

    match vfs::sync_file_page_cache() {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
    }
}
