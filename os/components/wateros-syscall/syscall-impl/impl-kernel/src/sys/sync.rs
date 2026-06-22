//! `sync(2)` / `fsync(2)` / `fdatasync(2)`：将脏数据刷回后端。

use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::vfs_util::vfs_error_to_errno;

fn sync_fd(op : &str, fd : usize) -> UserRet {
    log::info!("[sync-probe][sys_{op}] begin fd={}", fd);
    match vfs::fd::with_current_io(fd, |handle| handle.flush()) {
        Ok(()) => {
            log::info!("[sync-probe][sys_{op}] end fd={} ret=0", fd);
            UserRet::from_success(0)
        }
        Err(err) => {
            let errno = vfs_error_to_errno(err);
            log::info!("[sync-probe][sys_{op}] end fd={} err={:?} errno={:?}",
                       fd,
                       err,
                       errno);
            UserRet::from_error(errno)
        }
    }
}

pub(crate) fn sys_fsync(args : SyscallArgs) -> UserRet { sync_fd("fsync", args.arg(0)) }

pub(crate) fn sys_fdatasync(args : SyscallArgs) -> UserRet { sync_fd("fdatasync", args.arg(0)) }

pub(crate) fn sys_sync(_args : SyscallArgs) -> UserRet {
    // Linux sync(2) 发起系统级写回并始终返回 0；单个写回错误由后续文件操作报告。
    log::info!("[sync-probe][sys_sync] begin flush_all_open_files");
    let _ = vfs::fd::flush_all_open_files();
    log::info!("[sync-probe][sys_sync] end ret=0");
    UserRet::from_success(0)
}
