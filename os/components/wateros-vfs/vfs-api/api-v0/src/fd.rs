//! per-task 文件描述符会话（实现见 `impl-fd-session` / `wateros-vfs::fd`）。

extern crate alloc;

use alloc::boxed::Box;

use crate::error::{VfsError, VfsResult};
use crate::handle::VfsIoHandle;

/// 标准输入 fd 号。
pub const VFS_STDIN_FD: usize = 0;
/// 标准输出 fd 号。
pub const VFS_STDOUT_FD: usize = 1;
/// 标准错误 fd 号。
pub const VFS_STDERR_FD: usize = 2;
/// 首个可动态分配的 fd 号。
pub const VFS_FIRST_DYNAMIC_FD: usize = 3;

/// 任务内 fd 编号（对外 ABI 编号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VfsFd(pub i32);

/// 每任务 fd 表：分配、查找、关闭（对象级会话，非静态后端）。
pub trait VfsFdSession {
    fn get_io(&mut self, fd: usize) -> VfsResult<&mut (dyn VfsIoHandle + '_)>;

    fn alloc_fd(&mut self, handle: Box<dyn VfsIoHandle>) -> VfsResult<usize> {
        let _ = handle;
        Err(VfsError::Unsupported)
    }

    fn close_fd(&mut self, fd: usize) -> VfsResult<()> {
        let _ = fd;
        Err(VfsError::BadFd)
    }
}
