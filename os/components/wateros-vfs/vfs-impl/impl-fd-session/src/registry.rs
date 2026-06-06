//! 按 [`task::TaskId`] 索引的 per-task fd 表。

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use api_v0::{VfsError, VfsFdSession, VfsIoHandle, VfsResult, VFS_FIRST_DYNAMIC_FD,
    VFS_STDERR_FD, VFS_STDIN_FD, VFS_STDOUT_FD,
};
use driver_character_api_v0::character_device_at;

use crate::char_dev_handle::CharDevHandle;
use crate::handles::{ConsoleInHandle, ConsoleOutHandle};

/// Linux `FD_CLOEXEC`（`fcntl` / `dup3`）。
pub const FD_CLOEXEC: u8 = 1;

/// 全局 per-task fd 注册表。
pub struct PerTaskFdRegistry {
    tables: Vec<Vec<Option<Box<dyn VfsIoHandle>>>>,
    fd_flags: Vec<Vec<u8>>,
    owners: Vec<Option<task::TaskId>>,
    ref_counts: Vec<usize>,
}

impl PerTaskFdRegistry {
    pub const fn new() -> Self {
        Self {
            tables: Vec::new(),
            fd_flags: Vec::new(),
            owners: Vec::new(),
            ref_counts: Vec::new(),
        }
    }

    fn ensure_task(&mut self, task_id: task::TaskId) {
        if self.tables.len() <= task_id {
            self.tables.resize_with(task_id + 1, Vec::new);
            self.fd_flags.resize_with(task_id + 1, Vec::new);
            self.owners.resize_with(task_id + 1, || None);
            self.ref_counts.resize(task_id + 1, 0);
        }
        if self.owners[task_id].is_none() {
            self.owners[task_id] = Some(task_id);
            self.ref_counts[task_id] = 1;
        }
        let owner = self.effective_owner(task_id);
        let table = &mut self.tables[owner];
        if table.len() < VFS_FIRST_DYNAMIC_FD {
            table.resize_with(VFS_FIRST_DYNAMIC_FD, || None);
            table[VFS_STDIN_FD] = Some(default_stdin_handle());
            table[VFS_STDOUT_FD] = Some(default_stdout_handle());
            table[VFS_STDERR_FD] = Some(default_stdout_handle());
            let flags = &mut self.fd_flags[owner];
            if flags.len() < VFS_FIRST_DYNAMIC_FD {
                flags.resize(VFS_FIRST_DYNAMIC_FD, 0);
            }
        }
    }

    fn effective_owner(&self, task_id: task::TaskId) -> task::TaskId {
        self.owners
            .get(task_id)
            .and_then(|owner| *owner)
            .unwrap_or(task_id)
    }

    fn table_mut(&mut self, task_id: task::TaskId) -> &mut Vec<Option<Box<dyn VfsIoHandle>>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        &mut self.tables[owner]
    }

    fn ensure_flags_len(&mut self, task_id: task::TaskId, len: usize) {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let flags = &mut self.fd_flags[owner];
        if flags.len() < len {
            flags.resize(len, 0);
        }
    }

    fn close_slot(&mut self, task_id: task::TaskId, fd: usize) -> VfsResult<()> {
        let mut handle = self.take_fd_for_close(task_id, fd)?;
        handle.close()?;
        Ok(())
    }

    fn take_table_handles(&mut self, owner: task::TaskId) -> Vec<Box<dyn VfsIoHandle>> {
        let mut handles = Vec::new();
        if let Some(table) = self.tables.get_mut(owner) {
            for slot in table.iter_mut() {
                if let Some(handle) = slot.take() {
                    handles.push(handle);
                }
            }
            table.clear();
        }
        if let Some(flags) = self.fd_flags.get_mut(owner) {
            flags.clear();
        }
        handles
    }

    pub fn take_fd_for_close(
        &mut self,
        task_id: task::TaskId,
        fd: usize,
    ) -> VfsResult<Box<dyn VfsIoHandle>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let handle = self.tables[owner]
            .get_mut(fd)
            .ok_or(VfsError::BadFd)?
            .take()
            .ok_or(VfsError::BadFd)?;
        if fd < self.fd_flags[owner].len() {
            self.fd_flags[owner][fd] = 0;
        }
        Ok(handle)
    }

    pub fn take_cloexec_fds_for_task(
        &mut self,
        task_id: task::TaskId,
    ) -> Vec<Box<dyn VfsIoHandle>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables[owner].len();
        let flags_len = self.fd_flags[owner].len();
        let mut handles = Vec::new();
        for fd in (0..table_len).rev() {
            let cloexec = fd < flags_len && (self.fd_flags[owner][fd] & FD_CLOEXEC) != 0;
            if cloexec {
                if let Ok(handle) = self.take_fd_for_close(task_id, fd) {
                    handles.push(handle);
                }
            }
        }
        handles
    }

    pub fn drain_task_fd_table(&mut self, task_id: task::TaskId) -> Vec<Box<dyn VfsIoHandle>> {
        let Some(owner) = self.release_owner(task_id) else {
            return Vec::new();
        };
        let mut handles = if self.ref_counts.get(owner).copied().unwrap_or(0) == 0 {
            self.take_table_handles(owner)
        } else {
            Vec::new()
        };
        if task_id != owner {
            handles.extend(self.take_table_handles(task_id));
        }
        handles
    }

    fn release_owner(&mut self, task_id: task::TaskId) -> Option<task::TaskId> {
        let owner = self.owners.get_mut(task_id)?.take()?;
        if owner < self.ref_counts.len() && self.ref_counts[owner] > 0 {
            self.ref_counts[owner] -= 1;
        }
        Some(owner)
    }

    fn close_table(&mut self, owner: task::TaskId) {
        let _ = self.take_table_handles(owner);
    }
}

impl VfsFdSession for PerTaskFdRegistry {
    fn get_io(&mut self, fd: usize) -> VfsResult<&mut (dyn VfsIoHandle + '_)> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        match self.table_mut(task_id).get_mut(fd) {
            Some(Some(h)) => Ok(h.as_mut()),
            _ => Err(VfsError::BadFd),
        }
    }

    fn alloc_fd(&mut self, handle: Box<dyn VfsIoHandle>) -> VfsResult<usize> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        let newfd = {
            let table = self.table_mut(task_id);
            if let Some(fd) = (VFS_FIRST_DYNAMIC_FD..table.len()).find(|&fd| table[fd].is_none())
            {
                table[fd] = Some(handle);
                fd
            } else {
                table.push(Some(handle));
                table.len() - 1
            }
        };
        let owner = self.effective_owner(task_id);
        self.ensure_flags_len(task_id, self.tables[owner].len());
        Ok(newfd)
    }

    fn close_fd(&mut self, fd: usize) -> VfsResult<()> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        self.close_fd_for_task(task_id, fd)
    }
}

impl PerTaskFdRegistry {
    /// 为指定任务分配 fd（`pipe2` 等可在已知 `task_id` 下使用）。
    pub fn alloc_fd_for_task(
        &mut self,
        task_id: task::TaskId,
        handle: Box<dyn VfsIoHandle>,
    ) -> usize {
        let (newfd, len) = {
            let table = self.table_mut(task_id);
            if let Some(fd) = (VFS_FIRST_DYNAMIC_FD..table.len()).find(|&fd| table[fd].is_none())
            {
                table[fd] = Some(handle);
                (fd, table.len())
            } else {
                table.push(Some(handle));
                let nf = table.len() - 1;
                (nf, table.len())
            }
        };
        self.ensure_flags_len(task_id, len);
        newfd
    }

    /// 按任务与 fd 号取可变句柄。
    pub fn get_io_for_task(
        &mut self,
        task_id: task::TaskId,
        fd: usize,
    ) -> VfsResult<&mut (dyn VfsIoHandle + '_)> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        match self.tables[owner].get_mut(fd) {
            Some(Some(h)) => Ok(h.as_mut()),
            _ => Err(VfsError::BadFd),
        }
    }

    /// 临时取出指定 fd 的句柄，让调用方可在不持有 fd 注册表借用时执行 I/O。
    pub fn take_io_for_task(
        &mut self,
        task_id: task::TaskId,
        fd: usize,
    ) -> VfsResult<Box<dyn VfsIoHandle>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        match self.tables[owner].get_mut(fd) {
            Some(slot @ Some(_)) => Ok(slot.take().expect("checked Some")),
            _ => Err(VfsError::BadFd),
        }
    }

    /// 将 [`take_io_for_task`] 取出的句柄放回原 fd 槽位。
    pub fn restore_io_for_task(
        &mut self,
        task_id: task::TaskId,
        fd: usize,
        handle: Box<dyn VfsIoHandle>,
    ) -> VfsResult<()> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table = &mut self.tables[owner];
        if fd >= table.len() {
            return Err(VfsError::BadFd);
        }
        if table[fd].is_some() {
            return Err(VfsError::BadFd);
        }
        table[fd] = Some(handle);
        Ok(())
    }

    /// 按任务关闭 fd；关闭时调用句柄的 `close`。
    pub fn close_fd_for_task(&mut self, task_id: task::TaskId, fd: usize) -> VfsResult<()> {
        self.close_slot(task_id, fd)
    }

    /// `dup(oldfd)`：复制到 ≥ `minfd` 的最低可用 fd。
    pub fn dup_fd_for_task(
        &mut self,
        task_id: task::TaskId,
        oldfd: usize,
        minfd: usize,
    ) -> VfsResult<usize> {
        let dup_handle = {
            let handle = self.get_io_for_task(task_id, oldfd)?;
            handle.duplicate()?
        };
        self.ensure_task(task_id);
        let newfd = {
            let owner = self.effective_owner(task_id);
            let table = &mut self.tables[owner];
            while table.len() < minfd {
                table.push(None);
            }
            if let Some(fd) = (minfd..table.len()).find(|&fd| table[fd].is_none()) {
                table[fd] = Some(dup_handle);
                fd
            } else {
                table.push(Some(dup_handle));
                table.len() - 1
            }
        };
        let owner = self.effective_owner(task_id);
        self.ensure_flags_len(task_id, self.tables[owner].len());
        self.fd_flags[owner][newfd] = 0;
        Ok(newfd)
    }

    /// `dup3(oldfd, newfd, cloexec)`。
    pub fn dup3_fd_for_task(
        &mut self,
        task_id: task::TaskId,
        oldfd: usize,
        newfd: usize,
        cloexec: bool,
    ) -> VfsResult<usize> {
        if oldfd == newfd {
            self.get_io_for_task(task_id, oldfd)?;
            if cloexec {
                self.set_fd_cloexec(task_id, newfd, true)?;
            }
            return Ok(newfd);
        }

        let dup_handle = {
            let handle = self.get_io_for_task(task_id, oldfd)?;
            handle.duplicate()?
        };

        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        if newfd < self.tables[owner].len() && self.tables[owner][newfd].is_some() {
            self.close_slot(task_id, newfd)?;
        }
        {
            let table = &mut self.tables[owner];
            while table.len() <= newfd {
                table.push(None);
            }
            table[newfd] = Some(dup_handle);
        }
        self.ensure_flags_len(task_id, self.tables[owner].len());
        self.fd_flags[owner][newfd] = if cloexec { FD_CLOEXEC } else { 0 };
        Ok(newfd)
    }

    /// `fcntl(F_GETFD)`。
    pub fn get_fd_flags(&mut self, task_id: task::TaskId, fd: usize) -> VfsResult<usize> {
        self.get_io_for_task(task_id, fd)?;
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let flags = &self.fd_flags[owner];
        let v = if fd < flags.len() { flags[fd] } else { 0 };
        Ok(usize::from(v & FD_CLOEXEC))
    }

    /// `fcntl(F_SETFD)`：当前仅支持 `FD_CLOEXEC` 位。
    pub fn set_fd_flags(&mut self, task_id: task::TaskId, fd: usize, val: usize) -> VfsResult<()> {
        self.get_io_for_task(task_id, fd)?;
        let cloexec = (val & usize::from(FD_CLOEXEC)) != 0;
        self.set_fd_cloexec(task_id, fd, cloexec)
    }

    fn set_fd_cloexec(&mut self, task_id: task::TaskId, fd: usize, cloexec: bool) -> VfsResult<()> {
        self.ensure_flags_len(task_id, fd + 1);
        let owner = self.effective_owner(task_id);
        self.fd_flags[owner][fd] = if cloexec { FD_CLOEXEC } else { 0 };
        Ok(())
    }

    /// fork 时初始化子任务 fd 表：仅默认 stdin/stdout/stderr（spawn 路径）。
    pub fn init_child_fd_table(&mut self, child: task::TaskId) {
        let _ = self.table_mut(child);
    }

    /// fork 时复制父任务 fd 表（含 pipe/文件等动态 fd）。
    pub fn copy_fd_table_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        self.ensure_task(parent);
        if self.owners.get(child).and_then(|owner| *owner).is_some() {
            self.drop_task_fd_table(child);
        }
        let parent_owner = self.effective_owner(parent);
        let parent_len = self.tables[parent_owner].len();
        let parent_flags = self.fd_flags[parent_owner].clone();

        let mut entries: Vec<(usize, Box<dyn VfsIoHandle>)> = Vec::new();
        for (fd, slot) in self.tables[parent_owner].iter().enumerate() {
            if let Some(handle) = slot {
                if let Ok(dup) = handle.duplicate() {
                    entries.push((fd, dup));
                }
            }
        }

        self.ensure_task(child);
        self.tables[child].clear();
        self.fd_flags[child].clear();

        for (fd, dup) in entries {
            let table = &mut self.tables[child];
            while table.len() <= fd {
                table.push(None);
            }
            table[fd] = Some(dup);
        }

        self.ensure_flags_len(child, parent_len.max(self.tables[child].len()));
        for fd in 0..parent_len.min(parent_flags.len()) {
            self.fd_flags[child][fd] = parent_flags[fd];
        }
    }

    /// thread clone 时共享父任务 fd 表。
    pub fn share_fd_table_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        self.ensure_task(parent);
        if self.owners.get(child).and_then(|owner| *owner).is_some() {
            self.drop_task_fd_table(child);
        }
        if self.tables.len() <= child {
            self.tables.resize_with(child + 1, Vec::new);
            self.fd_flags.resize_with(child + 1, Vec::new);
            self.owners.resize_with(child + 1, || None);
            self.ref_counts.resize(child + 1, 0);
        }
        let owner = self.effective_owner(parent);
        self.owners[child] = Some(owner);
        if owner >= self.ref_counts.len() {
            self.ref_counts.resize(owner + 1, 0);
        }
        self.ref_counts[owner] = self.ref_counts[owner].saturating_add(1);
    }

    /// `execve` 前关闭带 `FD_CLOEXEC` 的 fd。
    pub fn close_cloexec_fds_for_task(&mut self, task_id: task::TaskId) {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables[owner].len();
        let flags_len = self.fd_flags[owner].len();
        for fd in (0..table_len).rev() {
            let cloexec = fd < flags_len && (self.fd_flags[owner][fd] & FD_CLOEXEC) != 0;
            if cloexec {
                let _ = self.close_slot(task_id, fd);
            }
        }
    }

    /// 任务退出后关闭全部 fd 并清空槽位。
    pub fn drop_task_fd_table(&mut self, task_id: task::TaskId) {
        let Some(owner) = self.release_owner(task_id) else {
            return;
        };
        if self.ref_counts.get(owner).copied().unwrap_or(0) == 0 {
            self.close_table(owner);
        }
        if task_id != owner {
            self.tables[task_id].clear();
            self.fd_flags[task_id].clear();
        }
    }
}

fn default_stdin_handle() -> Box<dyn VfsIoHandle> {
    if let Some(dev) = character_device_at(0) {
        Box::new(CharDevHandle::new_stdin(dev))
    } else {
        Box::new(ConsoleInHandle)
    }
}

fn default_stdout_handle() -> Box<dyn VfsIoHandle> {
    if let Some(dev) = character_device_at(0) {
        Box::new(CharDevHandle::new_stdout(dev))
    } else {
        Box::new(ConsoleOutHandle)
    }
}
