//! `fadvise64(2)` 文件访问模式提示的兼容实现。

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use vfs::api::{VfsError, VfsNodeType};

const POSIX_FADV_NORMAL : usize = 0;
const POSIX_FADV_RANDOM : usize = 1;
const POSIX_FADV_SEQUENTIAL : usize = 2;
const POSIX_FADV_WILLNEED : usize = 3;
const POSIX_FADV_DONTNEED : usize = 4;
const POSIX_FADV_NOREUSE : usize = 5;

pub(crate) fn sys_fadvise64(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let offset = args.arg(1) as isize;
    let length = args.arg(2) as isize;
    let advice = args.arg(3);

    if offset < 0 || length < 0 || !valid_advice(advice) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    match vfs::fd::is_path_only_fd(fd) {
        Ok(true) => return UserRet::from_error(ErrNo::EBADF),
        Ok(false) => {}
        Err(VfsError::BadFd) => return UserRet::from_error(ErrNo::EBADF),
        Err(_) => return UserRet::from_error(ErrNo::EBADF),
    }
    match vfs::fd::with_current_io(fd, |handle| handle.metadata()) {
        Ok(meta) if meta.node_type == VfsNodeType::File => UserRet::from_success(0),
        Ok(_) => UserRet::from_error(ErrNo::ESPIPE),
        Err(VfsError::BadFd) => UserRet::from_error(ErrNo::EBADF),
        Err(_) => UserRet::from_error(ErrNo::ESPIPE),
    }
}

/// `readahead(fd, offset, count)` — 请求提前把普通文件区间装入页缓存。
///
/// 当前页缓存没有异步预读队列，因此把该调用作为可忽略的性能提示处理；仍完整
/// 校验 fd、访问模式、偏移和节点类型，避免错误地让目录、管道或只写 fd 成功。
pub(crate) fn sys_readahead(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let offset = args.arg(1) as isize;
    let _count = args.arg(2);

    if offset < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    match vfs::fd::is_path_only_fd(fd) {
        Ok(true) => return UserRet::from_error(ErrNo::EBADF),
        Ok(false) => {}
        Err(_) => return UserRet::from_error(ErrNo::EBADF),
    }

    match vfs::fd::with_current_io(fd, |handle| {
              const O_ACCMODE : u32 = 3;
              const O_WRONLY : u32 = 1;
              if handle.open_accmode() & O_ACCMODE == O_WRONLY {
                  return Err(VfsError::BadFd);
              }
              handle.metadata()
          }) {
        Ok(meta) if meta.node_type == VfsNodeType::File => UserRet::from_success(0),
        Ok(_) => UserRet::from_error(ErrNo::EINVAL),
        Err(VfsError::BadFd) => UserRet::from_error(ErrNo::EBADF),
        Err(_) => UserRet::from_error(ErrNo::EINVAL),
    }
}

#[inline]
fn valid_advice(advice : usize) -> bool {
    matches!(advice,
             POSIX_FADV_NORMAL |
             POSIX_FADV_RANDOM |
             POSIX_FADV_SEQUENTIAL |
             POSIX_FADV_WILLNEED |
             POSIX_FADV_DONTNEED |
             POSIX_FADV_NOREUSE)
}
