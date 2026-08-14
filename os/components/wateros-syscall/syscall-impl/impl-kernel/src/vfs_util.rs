//! [`VfsError`] 到 ABI [`ErrNo`] 的映射。
//! 本模块代码由AI完成

use api_v0::ErrNo;
use vfs::api::{VfsError, VfsOpenFlags};

// 本方法代码由AI完成
pub(crate) fn vfs_error_to_errno(err : VfsError) -> ErrNo {
    match err {
        VfsError::BadFd => ErrNo::EBADF,
        VfsError::Busy => ErrNo::EBUSY,
        VfsError::WouldBlock => ErrNo::EAGAIN,
        VfsError::Interrupted => ErrNo::EINTR,
        VfsError::BrokenPipe => ErrNo::EPIPE,
        VfsError::TooManyOpenFiles => ErrNo::EMFILE,
        VfsError::NoTask => ErrNo::ESRCH,
        VfsError::InvalidPath => ErrNo::EINVAL,
        VfsError::Unsupported => ErrNo::EOPNOTSUPP,
        VfsError::NoSpace => ErrNo::ENOSPC,
        VfsError::NoMemory => ErrNo::ENOMEM,
        VfsError::Exists => ErrNo::EEXIST,
        VfsError::NotEmpty => ErrNo::ENOTEMPTY,
        VfsError::ReadOnlyFs => ErrNo::EROFS,
        VfsError::NotFound => ErrNo::ENOENT,
        VfsError::NoDevice => ErrNo::ENXIO,
        VfsError::AccessDenied => ErrNo::EACCES,
        VfsError::OperationNotPermitted => ErrNo::EPERM,
        VfsError::NotDirectory => ErrNo::ENOTDIR,
        VfsError::TooManySymlinks => ErrNo::ELOOP,
        VfsError::NotAFile => ErrNo::EISDIR,
        VfsError::NotMounted |
        VfsError::Driver |
        VfsError::Corrupt |
        VfsError::Io |
        VfsError::NotUtf8 => ErrNo::EIO,
    }
}

/// `pread`/`pwrite`/`sendfile` 路径：不可 seek 的句柄（pipe/socket 等）→ `ESPIPE`。
// 本方法代码由AI完成
pub(crate) fn vfs_io_at_error_to_errno(err : VfsError) -> ErrNo {
    match err {
        VfsError::Unsupported => ErrNo::ESPIPE,
        other => vfs_error_to_errno(other),
    }
}

/// Linux `openat(2)` flags → [`VfsOpenFlags`]。
// 本方法代码由AI完成
pub(crate) fn linux_open_flags_to_vfs(flags : u32) -> VfsOpenFlags {
    const O_ACCMODE : u32 = 3;
    const O_WRONLY : u32 = 1;
    const O_RDWR : u32 = 2;
    const O_CREAT : u32 = 0o100;
    const O_TRUNC : u32 = 0o1000;
    const O_APPEND : u32 = 0o2000;
    const O_NONBLOCK : u32 = 0o4000;
    const O_DIRECTORY : u32 = 0o200_000;

    let mut vf = VfsOpenFlags(0);
    match flags & O_ACCMODE {
        O_WRONLY => vf.0 |= VfsOpenFlags::WRITE,
        O_RDWR => vf.0 |= VfsOpenFlags::READ | VfsOpenFlags::WRITE,
        _ => vf.0 |= VfsOpenFlags::READ,
    }
    if flags & O_CREAT != 0 {
        vf.0 |= VfsOpenFlags::CREATE;
    }
    if flags & O_TRUNC != 0 {
        vf.0 |= VfsOpenFlags::TRUNC;
    }
    if flags & O_APPEND != 0 {
        vf.0 |= VfsOpenFlags::APPEND;
    }
    if flags & O_DIRECTORY != 0 {
        vf.0 |= VfsOpenFlags::DIRECTORY;
    }
    if flags & O_NONBLOCK != 0 {
        vf.0 |= VfsOpenFlags::NONBLOCK;
    }
    vf
}
