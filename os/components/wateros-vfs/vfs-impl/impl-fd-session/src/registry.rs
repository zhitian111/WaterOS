//! 以 [`task::TaskId`] 为 key 的 per-task fd 表。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;

use spin::Mutex;

use api_v0::{VfsError, VfsFdSession, VfsIoHandle, VfsResult, VFS_FIRST_DYNAMIC_FD,
    VFS_STDERR_FD, VFS_STDIN_FD, VFS_STDOUT_FD,
};
use driver_character_api_v0::{
    character_device_at, character_device_count, character_device_kind_at, CharacterDeviceKind,
    SharedCharacterDevice,
};

use crate::char_dev_handle::CharDevHandle;
use crate::handles::{ConsoleInHandle, ConsoleOutHandle};

/// Linux `FD_CLOEXEC`（`fcntl` / `dup3`）。
pub const FD_CLOEXEC: u8 = 1;
/// `O_PATH` 句柄：仅用于路径解析，不可用于读写/socket 操作。
pub const FD_PATH_ONLY: u8 = 2;

/// 全局 per-task fd 注册表。
// 本结构代码由AI完成
pub struct PerTaskFdRegistry {
    tables: BTreeMap<task::TaskId, Vec<Option<Box<dyn VfsIoHandle>>>>,
    fd_flags: BTreeMap<task::TaskId, Vec<u8>>,
    owners: BTreeMap<task::TaskId, task::TaskId>,
    ref_counts: BTreeMap<task::TaskId, usize>,
    /// 共享 fd 表：各槽位上正在进行的 `with_current_io` 会话数。
    io_inflight: BTreeMap<task::TaskId, Vec<u32>>,
    /// 共享 fd 表：按槽位串行化并发 I/O。
    io_slot_locks: BTreeMap<task::TaskId, Vec<Arc<Mutex<()>>>>,
}

impl PerTaskFdRegistry {
    pub const fn new() -> Self {
        Self {
            tables: BTreeMap::new(),
            fd_flags: BTreeMap::new(),
            owners: BTreeMap::new(),
            ref_counts: BTreeMap::new(),
            io_inflight: BTreeMap::new(),
            io_slot_locks: BTreeMap::new(),
        }
    }

// 本方法代码由AI完成
    fn ensure_task(&mut self, task_id: task::TaskId) {
        self.owners.entry(task_id).or_insert(task_id);
        self.ref_counts.entry(task_id).or_insert(1);
        let owner = self.effective_owner(task_id);
        let table = self.tables.entry(owner).or_insert_with(Vec::new);
        if table.len() < VFS_FIRST_DYNAMIC_FD {
            table.resize_with(VFS_FIRST_DYNAMIC_FD, || None);
            table[VFS_STDIN_FD] = Some(default_stdin_handle());
            table[VFS_STDOUT_FD] = Some(default_stdout_handle());
            table[VFS_STDERR_FD] = Some(default_stdout_handle());
            let flags = self.fd_flags.entry(owner).or_insert_with(Vec::new);
            if flags.len() < VFS_FIRST_DYNAMIC_FD {
                flags.resize(VFS_FIRST_DYNAMIC_FD, 0);
            }
        }
    }

// 本方法代码由AI完成
    fn effective_owner(&self, task_id: task::TaskId) -> task::TaskId {
        self.owners
            .get(&task_id)
            .copied()
            .unwrap_or(task_id)
    }

// 本方法代码由AI完成
    fn ensure_shared_io_state(&mut self, owner: task::TaskId) {
        let len = self.tables.get(&owner).map(Vec::len).unwrap_or(0);
        let inflight = self.io_inflight.entry(owner).or_insert_with(Vec::new);
        if inflight.len() < len {
            inflight.resize(len, 0);
        }
        let locks = self.io_slot_locks.entry(owner).or_insert_with(Vec::new);
        while locks.len() < len {
            locks.push(Arc::new(Mutex::new(())));
        }
    }

// 本方法代码由AI完成
    fn ensure_fd_not_io_busy(&self, owner: task::TaskId, fd: usize) -> VfsResult<()> {
        if self
            .io_inflight
            .get(&owner)
            .and_then(|counts| counts.get(fd))
            .copied()
            .unwrap_or(0)
            > 0
        {
            return Err(VfsError::Busy);
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn table_mut(&mut self, task_id: task::TaskId) -> &mut Vec<Option<Box<dyn VfsIoHandle>>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        self.tables.get_mut(&owner).expect("fd table owner")
    }

// 本方法代码由AI完成
    fn ensure_flags_len(&mut self, task_id: task::TaskId, len: usize) {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let flags = self.fd_flags.entry(owner).or_insert_with(Vec::new);
        if flags.len() < len {
            flags.resize(len, 0);
        }
    }

// 本方法代码由AI完成
    fn close_slot(&mut self, task_id: task::TaskId, fd: usize) -> VfsResult<()> {
        let pid = task::process_task_snapshot(task_id).map(|snap| snap.pid);
        let mut handle = self.take_fd_for_close(task_id, fd)?;
        if let Some(pid) = pid {
            if let Ok(meta) = handle.metadata() {
                if let Some(key) = crate::file_lock::inode_key_from_metadata(&meta) {
                    crate::file_lock::release_process_inode_locks(pid, &key);
                    if let Some(owner) = handle.flock_owner_id() {
                        crate::file_lock::release_flock_owner(&key, owner);
                    }
                }
            }
        }
        handle.close()?;
        Ok(())
    }

// 本方法代码由AI完成
    fn take_table_handles(&mut self, owner: task::TaskId) -> Vec<Box<dyn VfsIoHandle>> {
        let mut handles = Vec::new();
        if let Some(mut table) = self.tables.remove(&owner) {
            for slot in table.iter_mut() {
                if let Some(handle) = slot.take() {
                    handles.push(handle);
                }
            }
        }
        self.fd_flags.remove(&owner);
        handles
    }

// 本方法代码由AI完成
    pub fn take_fd_for_close(
        &mut self,
        task_id: task::TaskId,
        fd: usize,
    ) -> VfsResult<Box<dyn VfsIoHandle>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        self.ensure_fd_not_io_busy(owner, fd)?;
        let handle = self.tables.get_mut(&owner)
            .ok_or(VfsError::BadFd)?
            .get_mut(fd)
            .ok_or(VfsError::BadFd)?
            .take()
            .ok_or(VfsError::BadFd)?;
        if let Some(flags) = self.fd_flags.get_mut(&owner) {
            if fd < flags.len() {
                flags[fd] = 0;
            }
        }
        Ok(handle)
    }

// 本方法代码由AI完成
    pub fn take_fd_range_for_close(
        &mut self,
        task_id: task::TaskId,
        first: usize,
        last: usize,
    ) -> VfsResult<Vec<(usize, Box<dyn VfsIoHandle>)>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables.get(&owner).map(Vec::len).unwrap_or(0);
        if first >= table_len {
            return Ok(Vec::new());
        }
        let end = last.min(table_len - 1);
        let mut handles = Vec::new();
        for fd in first..=end {
            if self.tables
                   .get(&owner)
                   .and_then(|table| table.get(fd))
                   .and_then(|slot| slot.as_ref())
                   .is_none()
            {
                continue;
            }
            self.ensure_fd_not_io_busy(owner, fd)?;
            let handle = self.tables
                             .get_mut(&owner)
                             .expect("fd table owner")
                             .get_mut(fd)
                             .expect("fd in range")
                             .take()
                             .expect("checked Some");
            if let Some(flags) = self.fd_flags.get_mut(&owner) {
                if fd < flags.len() {
                    flags[fd] = 0;
                }
            }
            handles.push((fd, handle));
        }
        Ok(handles)
    }

// 本方法代码由AI完成
    pub fn take_cloexec_fds_for_task(
        &mut self,
        task_id: task::TaskId,
    ) -> Vec<Box<dyn VfsIoHandle>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables.get(&owner).map(Vec::len).unwrap_or(0);
        let flags = self.fd_flags.get(&owner).cloned().unwrap_or_default();
        let mut handles = Vec::new();
        for fd in (0..table_len).rev() {
            let cloexec = fd < flags.len() && (flags[fd] & FD_CLOEXEC) != 0;
            if cloexec {
                if let Ok(handle) = self.take_fd_for_close(task_id, fd) {
                    handles.push(handle);
                }
            }
        }
        handles
    }

// 本方法代码由AI完成
    pub fn drain_task_fd_table(&mut self, task_id: task::TaskId) -> Vec<Box<dyn VfsIoHandle>> {
        let Some(owner) = self.release_owner(task_id) else {
            return Vec::new();
        };
        let mut handles = if self.ref_counts.get(&owner).copied().unwrap_or(0) == 0 {
            self.take_table_handles(owner)
        } else {
            Vec::new()
        };
        if task_id != owner {
            handles.extend(self.take_table_handles(task_id));
        }
        handles
    }

// 本方法代码由AI完成
    fn release_owner(&mut self, task_id: task::TaskId) -> Option<task::TaskId> {
        let owner = self.owners.remove(&task_id)?;
        if let Some(count) = self.ref_counts.get_mut(&owner) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.ref_counts.remove(&owner);
            }
        }
        Some(owner)
    }

// 本方法代码由AI完成
    fn close_table(&mut self, owner: task::TaskId) {
        let handles = self.take_table_handles(owner);
        for mut handle in handles {
            let _ = handle.close();
        }
    }

// 本方法代码由AI完成
    fn open_fd_count_for_task(&self, task_id: task::TaskId) -> usize {
        let owner = self.effective_owner(task_id);
        self.tables
            .get(&owner)
            .map(|table| table.iter().filter(|slot| slot.is_some()).count())
            .unwrap_or(0)
    }

    /// 调试面板用的全局 fd 注册表摘要。
    ///
    /// `task_bindings` 包含共享同一张 fd 表的任务；`table_count` 是实际独立 fd 表数。
    /// 调用方必须已经持有注册表锁。
    pub fn debug_counts(&self) -> (usize, usize, usize) {
        let open_fd_count = self.tables
                                .values()
                                .map(|table| table.iter().filter(|slot| slot.is_some()).count())
                                .sum();
        (self.owners.len(), self.tables.len(), open_fd_count)
    }

// 本方法代码由AI完成
    fn check_nofile_before_open(&self, task_id: task::TaskId) -> VfsResult<()> {
        let limit = task::nofile_rlimit_for_task(task_id);
        if self.open_fd_count_for_task(task_id) >= limit as usize {
            return Err(VfsError::TooManyOpenFiles);
        }
        Ok(())
    }
}

impl VfsFdSession for PerTaskFdRegistry {
// 本方法代码由AI完成
    fn get_io(&mut self, fd: usize) -> VfsResult<&mut (dyn VfsIoHandle + '_)> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        match self.table_mut(task_id).get_mut(fd) {
            Some(Some(h)) => Ok(h.as_mut()),
            _ => Err(VfsError::BadFd),
        }
    }

// 本方法代码由AI完成
    fn alloc_fd(&mut self, handle: Box<dyn VfsIoHandle>) -> VfsResult<usize> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        self.check_nofile_before_open(task_id)?;
        let newfd = {
            let table = self.table_mut(task_id);
            if let Some(fd) = (0..table.len()).find(|&fd| table[fd].is_none())
            {
                table[fd] = Some(handle);
                fd
            } else {
                table.push(Some(handle));
                table.len() - 1
            }
        };
        let owner = self.effective_owner(task_id);
        let len = self.tables.get(&owner).map(Vec::len).unwrap_or(0);
        self.ensure_flags_len(task_id, len);
        Ok(newfd)
    }

// 本方法代码由AI完成
    fn close_fd(&mut self, fd: usize) -> VfsResult<()> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        self.close_fd_for_task(task_id, fd)
    }
}

impl PerTaskFdRegistry {
    /// 为指定任务分配 fd（`pipe2` 等可在已知 `task_id` 下使用）。
// 本方法代码由AI完成
    pub fn alloc_fd_for_task(
        &mut self,
        task_id: task::TaskId,
        handle: Box<dyn VfsIoHandle>,
    ) -> VfsResult<usize> {
        self.check_nofile_before_open(task_id)?;
        let (newfd, len) = {
            let table = self.table_mut(task_id);
            if let Some(fd) = (0..table.len()).find(|&fd| table[fd].is_none())
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
        Ok(newfd)
    }

    /// 按任务与 fd 号取可变句柄。
// 本方法代码由AI完成
    pub fn get_io_for_task(
        &mut self,
        task_id: task::TaskId,
        fd: usize,
    ) -> VfsResult<&mut (dyn VfsIoHandle + '_)> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        match self.tables.get_mut(&owner).and_then(|table| table.get_mut(fd)) {
            Some(Some(h)) => Ok(h.as_mut()),
            _ => Err(VfsError::BadFd),
        }
    }

    /// 刷新全部实际 fd 表中的打开句柄；发生错误后仍继续其余写回。
// 本方法代码由AI完成
    pub fn flush_all(&mut self) -> VfsResult<()> {
        let mut first_error = None;
        for table in self.tables.values_mut() {
            for handle in table.iter_mut().flatten() {
                if let Err(err) = handle.flush() {
                    first_error.get_or_insert(err);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// 当前任务是否与其他任务共享同一 fd 表（如 `CLONE_FILES`）。
// 本方法代码由AI完成
    pub fn is_fd_table_shared(&self, task_id: task::TaskId) -> bool {
        let owner = self.effective_owner(task_id);
        self.ref_counts.get(&owner).copied().unwrap_or(0) > 1
    }

    /// 共享 fd 表路径：登记 I/O 会话并返回句柄指针与槽位锁（句柄仍留在表中）。
// 本方法代码由AI完成
    pub fn begin_shared_io_for_task(
        &mut self,
        task_id: task::TaskId,
        fd: usize,
    ) -> VfsResult<(*mut dyn VfsIoHandle, Arc<Mutex<()>>)> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        self.ensure_shared_io_state(owner);
        if self.tables.get(&owner).and_then(|t| t.get(fd)).and_then(|s| s.as_ref()).is_none() {
            return Err(VfsError::BadFd);
        }
        let inflight = self.io_inflight.get_mut(&owner).expect("shared io inflight");
        if fd >= inflight.len() {
            return Err(VfsError::BadFd);
        }
        inflight[fd] = inflight[fd].saturating_add(1);
        let handle_ptr = {
            let table = self.tables.get_mut(&owner).expect("fd table owner");
            let handle = table
                .get_mut(fd)
                .and_then(|slot| slot.as_mut())
                .ok_or(VfsError::BadFd)?;
            &mut **handle as *mut dyn VfsIoHandle
        };
        let slot_lock = self.io_slot_locks.get(&owner).expect("shared io locks")[fd].clone();
        Ok((handle_ptr, slot_lock))
    }

    /// 结束共享 fd 表上的 I/O 会话。
// 本方法代码由AI完成
    pub fn end_shared_io_for_task(&mut self, task_id: task::TaskId, fd: usize) {
        let owner = self.effective_owner(task_id);
        if let Some(inflight) = self.io_inflight.get_mut(&owner) {
            if fd < inflight.len() && inflight[fd] > 0 {
                inflight[fd] -= 1;
            }
        }
    }

    /// 临时取出指定 fd 的句柄，让调用方可在不持有 fd 注册表借用时执行 I/O。
// 本方法代码由AI完成
    pub fn take_io_for_task(
        &mut self,
        task_id: task::TaskId,
        fd: usize,
    ) -> VfsResult<Box<dyn VfsIoHandle>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        match self.tables.get_mut(&owner).and_then(|table| table.get_mut(fd)) {
            Some(slot @ Some(_)) => Ok(slot.take().expect("checked Some")),
            _ => Err(VfsError::BadFd),
        }
    }

    /// 将 [`take_io_for_task`] 取出的句柄放回原 fd 槽位。
// 本方法代码由AI完成
    pub fn restore_io_for_task(
        &mut self,
        task_id: task::TaskId,
        fd: usize,
        handle: Box<dyn VfsIoHandle>,
    ) -> VfsResult<()> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table = self.tables.get_mut(&owner).ok_or(VfsError::BadFd)?;
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
// 本方法代码由AI完成
    pub fn close_fd_for_task(&mut self, task_id: task::TaskId, fd: usize) -> VfsResult<()> {
        self.close_slot(task_id, fd)
    }

    /// `dup(oldfd)`：复制到 ≥ `minfd` 的最低可用 fd。
// 本方法代码由AI完成
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
        self.check_nofile_before_open(task_id)?;
        self.ensure_task(task_id);
        let newfd = {
            let owner = self.effective_owner(task_id);
            let table = self.tables.get_mut(&owner).expect("fd table owner");
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
        let len = self.tables.get(&owner).map(Vec::len).unwrap_or(0);
        self.ensure_flags_len(task_id, len);
        self.fd_flags.get_mut(&owner).expect("fd flags owner")[newfd] = 0;
        Ok(newfd)
    }

    /// `dup3(oldfd, newfd, cloexec)`。
// 本方法代码由AI完成
    pub fn dup3_fd_for_task(
        &mut self,
        task_id: task::TaskId,
        oldfd: usize,
        newfd: usize,
        cloexec: bool,
    ) -> VfsResult<(usize, Option<Box<dyn VfsIoHandle>>)> {
        if oldfd == newfd {
            self.get_io_for_task(task_id, oldfd)?;
            if cloexec {
                self.set_fd_cloexec(task_id, newfd, true)?;
            }
            return Ok((newfd, None));
        }

        let dup_handle = {
            let handle = self.get_io_for_task(task_id, oldfd)?;
            handle.duplicate()?
        };

        if newfd >= task::nofile_rlimit_for_task(task_id) as usize {
            return Err(VfsError::BadFd);
        }

        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let newfd_was_open = self.tables
            .get(&owner)
            .and_then(|table| table.get(newfd))
            .and_then(|slot| slot.as_ref())
            .is_some();
        if !newfd_was_open {
            self.check_nofile_before_open(task_id)?;
        }
        let displaced = if self.tables.get(&owner)
                                      .and_then(|table| table.get(newfd))
                                      .and_then(|slot| slot.as_ref())
                                      .is_some() {
            self.ensure_fd_not_io_busy(owner, newfd)?;
            Some(self.take_fd_for_close(task_id, newfd)?)
        } else {
            None
        };
        {
            let table = self.tables.get_mut(&owner).expect("fd table owner");
            while table.len() <= newfd {
                table.push(None);
            }
            table[newfd] = Some(dup_handle);
        }
        let len = self.tables.get(&owner).map(Vec::len).unwrap_or(0);
        self.ensure_flags_len(task_id, len);
        self.fd_flags.get_mut(&owner).expect("fd flags owner")[newfd] =
            if cloexec { FD_CLOEXEC } else { 0 };
        Ok((newfd, displaced))
    }

    /// `fcntl(F_GETFD)`。
// 本方法代码由AI完成
    pub fn get_fd_flags(&mut self, task_id: task::TaskId, fd: usize) -> VfsResult<usize> {
        self.get_io_for_task(task_id, fd)?;
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let Some(flags) = self.fd_flags.get(&owner) else {
            return Ok(0);
        };
        let v = if fd < flags.len() { flags[fd] } else { 0 };
        Ok(usize::from(v & FD_CLOEXEC))
    }

    /// `fcntl(F_SETFD)`：当前仅支持 `FD_CLOEXEC` 位。
// 本方法代码由AI完成
    pub fn set_fd_flags(&mut self, task_id: task::TaskId, fd: usize, val: usize) -> VfsResult<()> {
        self.get_io_for_task(task_id, fd)?;
        let cloexec = (val & usize::from(FD_CLOEXEC)) != 0;
        self.set_fd_cloexec(task_id, fd, cloexec)
    }

    /// 将 `fd` 标记为 `O_PATH` 句柄（仅路径解析，禁止读写）。
// 本方法代码由AI完成
    pub fn set_fd_path_only(&mut self, task_id: task::TaskId, fd: usize) -> VfsResult<()> {
        self.get_io_for_task(task_id, fd)?;
        self.ensure_flags_len(task_id, fd + 1);
        let owner = self.effective_owner(task_id);
        self.fd_flags.get_mut(&owner).expect("fd flags owner")[fd] |= FD_PATH_ONLY;
        Ok(())
    }

    /// 查询 `fd` 是否为 `O_PATH` 句柄。
// 本方法代码由AI完成
    pub fn is_fd_path_only(&mut self, task_id: task::TaskId, fd: usize) -> VfsResult<bool> {
        self.get_io_for_task(task_id, fd)?;
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let Some(flags) = self.fd_flags.get(&owner) else {
            return Ok(false);
        };
        Ok(fd < flags.len() && flags[fd] & FD_PATH_ONLY != 0)
    }

// 本方法代码由AI完成
    fn set_fd_cloexec(&mut self, task_id: task::TaskId, fd: usize, cloexec: bool) -> VfsResult<()> {
        self.ensure_flags_len(task_id, fd + 1);
        let owner = self.effective_owner(task_id);
        let slot = self.fd_flags.get_mut(&owner).expect("fd flags owner");
        if cloexec {
            slot[fd] |= FD_CLOEXEC;
        } else {
            slot[fd] &= !FD_CLOEXEC;
        }
        Ok(())
    }

// 本方法代码由AI完成
    pub fn set_fd_range_cloexec(
        &mut self,
        task_id: task::TaskId,
        first: usize,
        last: usize,
        cloexec: bool,
    ) -> VfsResult<()> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables.get(&owner).map(Vec::len).unwrap_or(0);
        if first >= table_len {
            return Ok(());
        }
        let end = last.min(table_len - 1);
        self.ensure_flags_len(task_id, end + 1);
        let flags = self.fd_flags.get_mut(&owner).expect("fd flags owner");
        for fd in first..=end {
            if self.tables
                   .get(&owner)
                   .and_then(|table| table.get(fd))
                   .and_then(|slot| slot.as_ref())
                   .is_none()
            {
                continue;
            }
            if cloexec {
                flags[fd] |= FD_CLOEXEC;
            } else {
                flags[fd] &= !FD_CLOEXEC;
            }
        }
        Ok(())
    }

    /// fork 时初始化子任务 fd 表：仅默认 stdin/stdout/stderr（spawn 路径）。
// 本方法代码由AI完成
    pub fn init_child_fd_table(&mut self, child: task::TaskId) {
        let _ = self.table_mut(child);
    }

    /// fork 时复制父任务 fd 表（含 pipe/文件等动态 fd）。
// 本方法代码由AI完成
    pub fn copy_fd_table_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        self.ensure_task(parent);
        if self.owners.contains_key(&child) {
            self.drop_task_fd_table(child);
        }
        let parent_owner = self.effective_owner(parent);
        let parent_len = self.tables.get(&parent_owner).map(Vec::len).unwrap_or(0);
        let parent_flags = self.fd_flags.get(&parent_owner).cloned().unwrap_or_default();

        let mut entries: Vec<(usize, Box<dyn VfsIoHandle>)> = Vec::new();
        if let Some(parent_table) = self.tables.get(&parent_owner) {
            for (fd, slot) in parent_table.iter().enumerate() {
                if let Some(handle) = slot {
                    if let Ok(dup) = handle.duplicate() {
                        entries.push((fd, dup));
                    }
                }
            }
        }

        self.ensure_task(child);
        self.tables.entry(child).or_insert_with(Vec::new).clear();
        self.fd_flags.entry(child).or_insert_with(Vec::new).clear();

        for (fd, dup) in entries {
            let table = self.tables.get_mut(&child).expect("child fd table");
            while table.len() <= fd {
                table.push(None);
            }
            table[fd] = Some(dup);
        }

        let child_len = self.tables.get(&child).map(Vec::len).unwrap_or(0);
        self.ensure_flags_len(child, parent_len.max(child_len));
        for fd in 0..parent_len.min(parent_flags.len()) {
            self.fd_flags.get_mut(&child).expect("child fd flags")[fd] = parent_flags[fd];
        }

    }

    /// thread clone 时共享父任务 fd 表。
// 本方法代码由AI完成
    pub fn share_fd_table_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        self.ensure_task(parent);
        if self.owners.contains_key(&child) {
            self.drop_task_fd_table(child);
        }
        let owner = self.effective_owner(parent);
        self.ensure_shared_io_state(owner);
        self.owners.insert(child, owner);
        let count = self.ref_counts.entry(owner).or_insert(0);
        *count = count.saturating_add(1);
    }

    /// `execve` 前关闭带 `FD_CLOEXEC` 的 fd。
// 本方法代码由AI完成
    pub fn close_cloexec_fds_for_task(&mut self, task_id: task::TaskId) {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables.get(&owner).map(Vec::len).unwrap_or(0);
        let flags = self.fd_flags.get(&owner).cloned().unwrap_or_default();
        for fd in (0..table_len).rev() {
            let cloexec = fd < flags.len() && (flags[fd] & FD_CLOEXEC) != 0;
            if cloexec {
                let _ = self.close_slot(task_id, fd);
            }
        }
    }

    /// 任务退出后关闭全部 fd 并清空槽位。
// 本方法代码由AI完成
    pub fn drop_task_fd_table(&mut self, task_id: task::TaskId) {
        let Some(owner) = self.release_owner(task_id) else {
            return;
        };
        if self.ref_counts.get(&owner).copied().unwrap_or(0) == 0 {
            self.close_table(owner);
        }
        if task_id != owner {
            self.tables.remove(&task_id);
            self.fd_flags.remove(&task_id);
        }
    }
}

// 本方法代码由AI完成
fn default_stdin_handle() -> Box<dyn VfsIoHandle> {
    if let Some(dev) = default_serial_device() {
        Box::new(CharDevHandle::new_stdin(dev))
    } else {
        Box::new(ConsoleInHandle)
    }
}

// 本方法代码由AI完成
fn default_stdout_handle() -> Box<dyn VfsIoHandle> {
    if let Some(dev) = default_serial_device() {
        Box::new(CharDevHandle::new_stdout(dev))
    } else {
        Box::new(ConsoleOutHandle)
    }
}

// 本方法代码由AI完成
fn default_serial_device() -> Option<SharedCharacterDevice> {
    (0..character_device_count())
        .find(|&idx| character_device_kind_at(idx) == Some(CharacterDeviceKind::Serial))
        .and_then(character_device_at)
}
