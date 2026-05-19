//! [`VfsError`] 到 ABI [`ErrNo`] 的映射。

use abi::errno::ErrNo;
use vfs::api::VfsError;

pub(crate) fn vfs_error_to_errno(err : VfsError) -> ErrNo {
    match err {
        VfsError::BadFd => ErrNo::EBADF,
        VfsError::WouldBlock => ErrNo::EAGAIN,
        VfsError::BrokenPipe => ErrNo::EPIPE,
        VfsError::NoTask => ErrNo::ESRCH,
        VfsError::InvalidPath | VfsError::Unsupported => ErrNo::EINVAL,
        VfsError::NotFound => ErrNo::ENOENT,
        VfsError::NotMounted |
        VfsError::Driver |
        VfsError::Corrupt |
        VfsError::Io |
        VfsError::NotAFile |
        VfsError::NotUtf8 => ErrNo::EIO,
    }
}
