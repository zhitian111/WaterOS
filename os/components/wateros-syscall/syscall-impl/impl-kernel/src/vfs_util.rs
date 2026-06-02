//! [`VfsError`] 到 ABI [`ErrNo`] 的映射。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use vfs::api::{VfsError, VfsNodeType, VfsOpenFlags, VfsSeekWhence};

pub(crate) fn vfs_error_to_errno(err: VfsError) -> ErrNo {
    match err {
        VfsError::BadFd => ErrNo::EBADF,
        VfsError::WouldBlock => ErrNo::EAGAIN,
        VfsError::BrokenPipe => ErrNo::EPIPE,
        VfsError::NoTask => ErrNo::ESRCH,
        VfsError::InvalidPath | VfsError::Unsupported => ErrNo::EINVAL,
        VfsError::Exists => ErrNo::EEXIST,
        VfsError::ReadOnlyFs => ErrNo::EROFS,
        VfsError::NotFound => ErrNo::ENOENT,
        VfsError::NotAFile => ErrNo::EISDIR,
        VfsError::NotMounted
        | VfsError::Driver
        | VfsError::Corrupt
        | VfsError::Io
        | VfsError::NotUtf8 => ErrNo::EIO,
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
    const O_DIRECTORY: u32 = 0o200_000;

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
    vf
}

/// 从当前任务 `fd` 的 `offset` 起读取 `len` 字节到内核缓冲（供文件 `mmap` 等使用）。
pub(crate) fn read_fd_bytes_at(fd: usize, offset: usize, len: usize) -> Result<Vec<u8>, ErrNo> {
    use vfs::api::VfsResult;
    if len == 0 {
        return Err(ErrNo::EINVAL);
    }
    vfs::fd::with_current_io(fd, |handle| -> VfsResult<Vec<u8>> {
        let meta = handle.metadata()?;
        if meta.node_type != VfsNodeType::File {
            return Err(VfsError::NotAFile);
        }
        let file_size = meta.size as usize;
        let end = offset
            .checked_add(len)
            .ok_or(VfsError::InvalidPath)?;
        if end > file_size {
            return Err(VfsError::InvalidPath);
        }
        let mut buf = Vec::with_capacity(len);
        buf.resize(len, 0);
        handle.seek(offset as i64, VfsSeekWhence::Set)?;
        let n = handle.read(&mut buf)?;
        if n != len {
            return Err(VfsError::Io);
        }
        Ok(buf)
    })
    .map_err(vfs_error_to_errno)
}
