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

/// 任务内 fd 编号（对外 ABI 编号，与 Linux `int` fd 对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VfsFd(pub i32);

/// 每任务 fd 表：分配、查找、关闭（对象级会话，非静态后端）。
pub trait VfsFdSession {
    /// 按 fd 号取可变 I/O 句柄；无效 fd 返回 [`VfsError::BadFd`]。
    fn get_io(&mut self, fd: usize) -> VfsResult<&mut (dyn VfsIoHandle + '_)>;

    /// 分配最低可用 fd 并绑定句柄；默认不支持。
    fn alloc_fd(&mut self, handle: Box<dyn VfsIoHandle>) -> VfsResult<usize> {
        let _ = handle;
        Err(VfsError::Unsupported)
    }

    /// 关闭 fd 并释放槽位；默认返回 [`VfsError::BadFd`]。
    fn close_fd(&mut self, fd: usize) -> VfsResult<()> {
        let _ = fd;
        Err(VfsError::BadFd)
    }
}
