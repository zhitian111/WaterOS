//! [`VfsError`] 到 ABI [`ErrNo`] 的映射。

use abi::errno::ErrNo;
use vfs::api::{VfsError, VfsOpenFlags};

pub(crate) fn vfs_error_to_errno(err : VfsError) -> ErrNo {
    match err {
        VfsError::BadFd => ErrNo::EBADF,
        VfsError::WouldBlock => ErrNo::EAGAIN,
        VfsError::BrokenPipe => ErrNo::EPIPE,
        VfsError::NoTask => ErrNo::ESRCH,
        VfsError::InvalidPath | VfsError::Unsupported => ErrNo::EINVAL,
        VfsError::NotFound => ErrNo::ENOENT,
        VfsError::NotAFile => ErrNo::EISDIR,
        VfsError::NotMounted |
        VfsError::Driver |
        VfsError::Corrupt |
        VfsError::Io |
        VfsError::NotUtf8 => ErrNo::EIO,
    }
}

/// Linux `openat(2)` flags → [`VfsOpenFlags`]。
pub(crate) fn linux_open_flags_to_vfs(flags: u32) -> VfsOpenFlags {
    const O_ACCMODE: u32 = 3;
    const O_WRONLY: u32 = 1;
    const O_RDWR: u32 = 2;
    const O_CREAT: u32 = 0o100;
    const O_TRUNC: u32 = 0o1000;
    const O_APPEND: u32 = 0o2000;

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
    vf
}
