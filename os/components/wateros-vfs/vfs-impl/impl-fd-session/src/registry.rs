//! 按 [`task::TaskId`] 索引的 per-task fd 表。

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use api_v0::{
    VfsError, VfsFdSession, VfsIoHandle, VfsResult, VFS_FIRST_DYNAMIC_FD, VFS_STDERR_FD,
    VFS_STDIN_FD, VFS_STDOUT_FD,
};

use crate::handles::{ConsoleInHandle, ConsoleOutHandle};

/// 全局 per-task fd 注册表。
pub struct PerTaskFdRegistry {
    tables : Vec<Vec<Option<Box<dyn VfsIoHandle>>>>,
}

impl PerTaskFdRegistry {
    pub const fn new() -> Self { Self { tables : Vec::new() } }

    fn table_mut(&mut self, task_id : task::TaskId) -> &mut Vec<Option<Box<dyn VfsIoHandle>>> {
        if self.tables.len() <= task_id {
            self.tables
                .resize_with(task_id + 1, Vec::new);
        }
        let table = &mut self.tables[task_id];
        if table.len() < VFS_FIRST_DYNAMIC_FD {
            table.resize_with(VFS_FIRST_DYNAMIC_FD, || None);
            table[VFS_STDIN_FD] = Some(Box::new(ConsoleInHandle));
            table[VFS_STDOUT_FD] = Some(Box::new(ConsoleOutHandle));
            table[VFS_STDERR_FD] = Some(Box::new(ConsoleOutHandle));
        }
        table
    }
}

impl VfsFdSession for PerTaskFdRegistry {
    fn get_io(&mut self, fd : usize) -> VfsResult<&mut (dyn VfsIoHandle + '_)> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        match self.table_mut(task_id)
                  .get_mut(fd)
        {
            Some(Some(h)) => Ok(h.as_mut()),
            _ => Err(VfsError::BadFd),
        }
    }

    fn alloc_fd(&mut self, handle : Box<dyn VfsIoHandle>) -> VfsResult<usize> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        let table = self.table_mut(task_id);
        for fd in VFS_FIRST_DYNAMIC_FD..table.len() {
            if table[fd].is_none() {
                table[fd] = Some(handle);
                return Ok(fd);
            }
        }
        table.push(Some(handle));
        Ok(table.len() - 1)
    }

    fn close_fd(&mut self, fd : usize) -> VfsResult<()> {
        if fd < VFS_FIRST_DYNAMIC_FD {
            return Err(VfsError::BadFd);
        }
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        let mut handle = self.table_mut(task_id)
                             .get_mut(fd)
                             .ok_or(VfsError::BadFd)?
                             .take()
                             .ok_or(VfsError::BadFd)?;
        handle.close()
    }
}

impl PerTaskFdRegistry {
    /// 为指定任务分配 fd（`pipe2` 等可在已知 `task_id` 下使用）。
    pub fn alloc_fd_for_task(&mut self,
                             task_id : task::TaskId,
                             handle : Box<dyn VfsIoHandle>)
                             -> usize {
        let table = self.table_mut(task_id);
        for fd in VFS_FIRST_DYNAMIC_FD..table.len() {
            if table[fd].is_none() {
                table[fd] = Some(handle);
                return fd;
            }
        }
        table.push(Some(handle));
        table.len() - 1
    }

    /// 按任务与 fd 号取可变句柄。
    pub fn get_io_for_task(&mut self,
                           task_id : task::TaskId,
                           fd : usize)
                           -> VfsResult<&mut (dyn VfsIoHandle + '_)> {
        match self.table_mut(task_id)
                  .get_mut(fd)
        {
            Some(Some(h)) => Ok(h.as_mut()),
            _ => Err(VfsError::BadFd),
        }
    }

    /// 按任务关闭 fd；关闭时调用句柄的 `close`。
    pub fn close_fd_for_task(&mut self, task_id : task::TaskId, fd : usize) -> VfsResult<()> {
        if fd < VFS_FIRST_DYNAMIC_FD {
            return Err(VfsError::BadFd);
        }
        let mut handle = self.table_mut(task_id)
                             .get_mut(fd)
                             .ok_or(VfsError::BadFd)?
                             .take()
                             .ok_or(VfsError::BadFd)?;
        handle.close()
    }

    /// fork 时初始化子任务 fd 表：创建独立的 stdin/stdout/stderr 控制台句柄。
    ///
    /// 动态 fd（≥3，pipe/file 等）不复制——当前 oscomp fork 测例子进程仅需 write+exit。
    pub fn init_child_fd_table(&mut self, child : task::TaskId) {
        // `table_mut` 会自动填充 fd 0/1/2 的默认控制台句柄
        let _ = self.table_mut(child);
    }
}
