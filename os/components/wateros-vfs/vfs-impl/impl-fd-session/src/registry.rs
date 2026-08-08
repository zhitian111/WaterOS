//! 以 [`task::TaskId`] 为 key 的 per-task fd 表。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;

use spin::Mutex;

use api_v0::{
    VfsError, VfsFdSession, VfsIoHandle, VfsPreparedRead, VfsResult, VFS_FIRST_DYNAMIC_FD,
    VFS_STDERR_FD, VFS_STDIN_FD, VFS_STDOUT_FD,
};
use driver_character_api_v0::{
    character_device_at, character_device_count, character_device_kind_at, CharacterDeviceKind,
    SharedCharacterDevice,
};

use crate::char_dev_handle::CharDevHandle;
use crate::handles::{ConsoleInHandle, ConsoleOutHandle};
use tty::{self, TtyControlEvent};

/// Linux `FD_CLOEXEC`（`fcntl` / `dup3`）。
pub const FD_CLOEXEC : u8 = 1;
/// `O_PATH` 句柄：仅用于路径解析，不可用于读写/socket 操作。
pub const FD_PATH_ONLY : u8 = 2;

struct OpenFileDescription {
    handle : Box<dyn VfsIoHandle>,
    closed : bool,
}

impl OpenFileDescription {
    fn new(handle : Box<dyn VfsIoHandle>) -> Self {
        Self { handle,
               closed : false }
    }

    fn close_once(&mut self) -> VfsResult<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.handle.close()
    }
}

impl Drop for OpenFileDescription {
    fn drop(&mut self) { let _ = self.close_once(); }
}

/// A stable fd-slot handle shared only by transient I/O leases.
#[derive(Clone)]
pub struct SharedIoHandle {
    inner : Arc<Mutex<OpenFileDescription>>,
    snapshot : Arc<Mutex<Option<OpenFileDescription>>>,
}

impl SharedIoHandle {
    pub fn new(handle : Box<dyn VfsIoHandle>) -> Self {
        let snapshot = handle.duplicate()
                             .ok()
                             .map(OpenFileDescription::new);
        Self { inner : Arc::new(Mutex::new(OpenFileDescription::new(handle))),
               snapshot : Arc::new(Mutex::new(snapshot)) }
    }

    pub fn with_io<R>(&self,
                      f : impl FnOnce(&mut (dyn VfsIoHandle + '_)) -> VfsResult<R>)
                      -> VfsResult<R> {
        let mut inner = self.inner.lock();
        f(inner.handle
               .as_mut())
    }

    /// Capture a prepared read while holding the fd-slot lock only briefly.
    pub fn prepare_read(&self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        let mut inner = self.inner.lock();
        inner.handle
             .prepare_read(max_len)
    }

    /// Create an independent fd-slot handle. If the live handle is blocked in
    /// I/O, duplicate the snapshot captured immediately before that I/O.
    pub fn duplicate(&self) -> VfsResult<Self> {
        let duplicate = if let Some(inner) = self.inner
                                                 .try_lock()
        {
            inner.handle
                 .duplicate()?
        } else {
            let snapshot = self.snapshot.lock();
            snapshot.as_ref()
                    .ok_or(VfsError::Busy)?
                    .handle
                    .duplicate()?
        };
        Ok(Self::new(duplicate))
    }

    /// Close immediately when this is the final reference. If an I/O lease is
    /// still active, `OpenFileDescription::drop` closes after that lease ends.
    pub fn close(self) -> VfsResult<()> {
        if Arc::strong_count(&self.inner) == 1 {
            self.inner
                .lock()
                .close_once()
        } else {
            Ok(())
        }
    }
}

/// 全局 per-task fd 注册表。
// 本结构代码由AI完成
pub struct PerTaskFdRegistry {
    tables : BTreeMap<task::TaskId, Vec<Option<SharedIoHandle>>>,
    fd_flags : BTreeMap<task::TaskId, Vec<u8>>,
    owners : BTreeMap<task::TaskId, task::TaskId>,
    ref_counts : BTreeMap<task::TaskId, usize>,
}

impl PerTaskFdRegistry {
    pub const fn new() -> Self {
        Self { tables : BTreeMap::new(),
               fd_flags : BTreeMap::new(),
               owners : BTreeMap::new(),
               ref_counts : BTreeMap::new() }
    }

    // 本方法代码由AI完成
    fn ensure_task(&mut self, task_id : task::TaskId) {
        if !self.owners
                .contains_key(&task_id)
        {
            self.owners
                .insert(task_id, task_id);
            self.ref_counts
                .insert(task_id, 1);
        }
        let owner = self.effective_owner(task_id);
        self.ref_counts
            .entry(owner)
            .or_insert(1);
        let table = self.tables
                        .entry(owner)
                        .or_insert_with(Vec::new);
        if table.len() < VFS_FIRST_DYNAMIC_FD {
            table.resize_with(VFS_FIRST_DYNAMIC_FD, || None);
            table[VFS_STDIN_FD] = Some(SharedIoHandle::new(default_stdin_handle()));
            table[VFS_STDOUT_FD] = Some(SharedIoHandle::new(default_stdout_handle()));
            table[VFS_STDERR_FD] = Some(SharedIoHandle::new(default_stdout_handle()));
            let flags = self.fd_flags
                            .entry(owner)
                            .or_insert_with(Vec::new);
            if flags.len() < VFS_FIRST_DYNAMIC_FD {
                flags.resize(VFS_FIRST_DYNAMIC_FD, 0);
            }
        }
    }

    // 本方法代码由AI完成
    fn effective_owner(&self, task_id : task::TaskId) -> task::TaskId {
        self.owners
            .get(&task_id)
            .copied()
            .unwrap_or(task_id)
    }

    // 本方法代码由AI完成
    fn table_mut(&mut self, task_id : task::TaskId) -> &mut Vec<Option<SharedIoHandle>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        self.tables
            .get_mut(&owner)
            .expect("fd table owner")
    }

    // 本方法代码由AI完成
    fn ensure_flags_len(&mut self, task_id : task::TaskId, len : usize) {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let flags = self.fd_flags
                        .entry(owner)
                        .or_insert_with(Vec::new);
        if flags.len() < len {
            flags.resize(len, 0);
        }
    }

    // 本方法代码由AI完成
    fn close_slot(&mut self, task_id : task::TaskId, fd : usize) -> VfsResult<()> {
        let pid = task::process_task_snapshot(task_id).map(|snap| snap.pid);
        let handle = self.take_fd_for_close(task_id, fd)?;
        if let Some(pid) = pid {
            let _ =
                handle.with_io(|io| {
                          if let Ok(meta) = io.metadata() {
                              if let Some(key) = crate::file_lock::inode_key_from_metadata(&meta) {
                                  crate::file_lock::release_process_inode_locks(pid, &key);
                                  if let Some(owner) = io.flock_owner_id() {
                                      crate::file_lock::release_flock_owner(&key, owner);
                                  }
                              }
                          }
                          Ok(())
                      });
        }
        handle.close()
    }

    // 本方法代码由AI完成
    fn take_table_handles(&mut self, owner : task::TaskId) -> Vec<SharedIoHandle> {
        let mut handles = Vec::new();
        if let Some(mut table) = self.tables
                                     .remove(&owner)
        {
            for slot in table.iter_mut() {
                if let Some(handle) = slot.take() {
                    handles.push(handle);
                }
            }
        }
        self.fd_flags
            .remove(&owner);
        handles
    }

    // 本方法代码由AI完成
    pub fn take_fd_for_close(&mut self,
                             task_id : task::TaskId,
                             fd : usize)
                             -> VfsResult<SharedIoHandle> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let handle = self.tables
                         .get_mut(&owner)
                         .ok_or(VfsError::BadFd)?
                         .get_mut(fd)
                         .ok_or(VfsError::BadFd)?
                         .take()
                         .ok_or(VfsError::BadFd)?;
        if let Some(flags) = self.fd_flags
                                 .get_mut(&owner)
        {
            if fd < flags.len() {
                flags[fd] = 0;
            }
        }
        Ok(handle)
    }

    // 本方法代码由AI完成
    pub fn take_fd_range_for_close(&mut self,
                                   task_id : task::TaskId,
                                   first : usize,
                                   last : usize)
                                   -> VfsResult<Vec<(usize, SharedIoHandle)>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables
                            .get(&owner)
                            .map(Vec::len)
                            .unwrap_or(0);
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
            let handle = self.tables
                             .get_mut(&owner)
                             .expect("fd table owner")
                             .get_mut(fd)
                             .expect("fd in range")
                             .take()
                             .expect("checked Some");
            if let Some(flags) = self.fd_flags
                                     .get_mut(&owner)
            {
                if fd < flags.len() {
                    flags[fd] = 0;
                }
            }
            handles.push((fd, handle));
        }
        Ok(handles)
    }

    // 本方法代码由AI完成
    pub fn take_cloexec_fds_for_task(&mut self,
                                     task_id : task::TaskId)
                                     -> Vec<(usize, SharedIoHandle)> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables
                            .get(&owner)
                            .map(Vec::len)
                            .unwrap_or(0);
        let flags = self.fd_flags
                        .get(&owner)
                        .cloned()
                        .unwrap_or_default();
        let mut handles = Vec::new();
        for fd in (0..table_len).rev() {
            let cloexec = fd < flags.len() && (flags[fd] & FD_CLOEXEC) != 0;
            if cloexec {
                if let Ok(handle) = self.take_fd_for_close(task_id, fd) {
                    handles.push((fd, handle));
                }
            }
        }
        handles
    }

    // 本方法代码由AI完成
    pub fn drain_task_fd_table(&mut self, task_id : task::TaskId) -> Vec<SharedIoHandle> {
        let Some(owner) = self.release_owner(task_id) else {
            return Vec::new();
        };
        let mut handles = if self.ref_counts
                                 .get(&owner)
                                 .copied()
                                 .unwrap_or(0) ==
                             0
        {
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
    fn release_owner(&mut self, task_id : task::TaskId) -> Option<task::TaskId> {
        let owner = self.owners
                        .remove(&task_id)?;
        if let Some(count) = self.ref_counts
                                 .get_mut(&owner)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.ref_counts
                    .remove(&owner);
            }
        }
        Some(owner)
    }

    // 本方法代码由AI完成
    fn close_table(&mut self, owner : task::TaskId) {
        let handles = self.take_table_handles(owner);
        for handle in handles {
            let _ = handle.close();
        }
    }

    // 本方法代码由AI完成
    fn open_fd_count_for_task(&self, task_id : task::TaskId) -> usize {
        let owner = self.effective_owner(task_id);
        self.tables
            .get(&owner)
            .map(|table| {
                table.iter()
                     .filter(|slot| slot.is_some())
                     .count()
            })
            .unwrap_or(0)
    }

    /// 调试面板用的全局 fd 注册表摘要。
    ///
    /// `task_bindings` 包含共享同一张 fd 表的任务；`table_count` 是实际独立 fd 表数。
    /// 调用方必须已经持有注册表锁。
    pub fn debug_counts(&self) -> (usize, usize, usize) {
        let open_fd_count = self.tables
                                .values()
                                .map(|table| {
                                    table.iter()
                                         .filter(|slot| slot.is_some())
                                         .count()
                                })
                                .sum();
        (self.owners.len(), self.tables.len(), open_fd_count)
    }

    // 本方法代码由AI完成
    fn check_nofile_before_open(&self, task_id : task::TaskId) -> VfsResult<()> {
        let limit = task::nofile_rlimit_for_task(task_id);
        if self.open_fd_count_for_task(task_id) >= limit as usize {
            return Err(VfsError::TooManyOpenFiles);
        }
        Ok(())
    }
}

impl VfsFdSession for PerTaskFdRegistry {
    // 本方法代码由AI完成
    fn get_io(&mut self, fd : usize) -> VfsResult<&mut (dyn VfsIoHandle + '_)> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        match self.table_mut(task_id)
                  .get_mut(fd)
        {
            Some(Some(h)) => {
                let inner = Arc::get_mut(&mut h.inner).ok_or(VfsError::Busy)?;
                Ok(inner.get_mut()
                        .handle
                        .as_mut())
            }
            _ => Err(VfsError::BadFd),
        }
    }

    // 本方法代码由AI完成
    fn alloc_fd(&mut self, handle : Box<dyn VfsIoHandle>) -> VfsResult<usize> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        self.check_nofile_before_open(task_id)?;
        let newfd = {
            let table = self.table_mut(task_id);
            if let Some(fd) = (0..table.len()).find(|&fd| table[fd].is_none()) {
                table[fd] = Some(SharedIoHandle::new(handle));
                fd
            } else {
                table.push(Some(SharedIoHandle::new(handle)));
                table.len() - 1
            }
        };
        let owner = self.effective_owner(task_id);
        let len = self.tables
                      .get(&owner)
                      .map(Vec::len)
                      .unwrap_or(0);
        self.ensure_flags_len(task_id, len);
        Ok(newfd)
    }

    // 本方法代码由AI完成
    fn close_fd(&mut self, fd : usize) -> VfsResult<()> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        self.close_fd_for_task(task_id, fd)
    }
}

impl PerTaskFdRegistry {
    /// 返回指定任务 fd 表中已占用的槽位，不创建缺失的 fd 表。
    pub fn open_fds_for_task(&self, task_id : task::TaskId) -> Vec<usize> {
        let owner = self.effective_owner(task_id);
        self.tables
            .get(&owner)
            .map(|table| {
                table.iter()
                     .enumerate()
                     .filter_map(|(fd, slot)| {
                         slot.as_ref()
                             .map(|_| fd)
                     })
                     .collect()
            })
            .unwrap_or_default()
    }

    /// 为指定任务分配 fd（`pipe2` 等可在已知 `task_id` 下使用）。
    // 本方法代码由AI完成
    pub fn alloc_fd_for_task(&mut self,
                             task_id : task::TaskId,
                             handle : Box<dyn VfsIoHandle>)
                             -> VfsResult<usize> {
        self.check_nofile_before_open(task_id)?;
        let (newfd, len) = {
            let table = self.table_mut(task_id);
            if let Some(fd) = (0..table.len()).find(|&fd| table[fd].is_none()) {
                table[fd] = Some(SharedIoHandle::new(handle));
                (fd, table.len())
            } else {
                table.push(Some(SharedIoHandle::new(handle)));
                let nf = table.len() - 1;
                (nf, table.len())
            }
        };
        self.ensure_flags_len(task_id, len);
        Ok(newfd)
    }

    /// 按任务与 fd 号取可变句柄。
    // 本方法代码由AI完成
    pub fn get_io_for_task(&mut self,
                           task_id : task::TaskId,
                           fd : usize)
                           -> VfsResult<&mut (dyn VfsIoHandle + '_)> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        match self.tables
                  .get_mut(&owner)
                  .and_then(|table| table.get_mut(fd))
        {
            Some(Some(h)) => {
                let inner = Arc::get_mut(&mut h.inner).ok_or(VfsError::Busy)?;
                Ok(inner.get_mut()
                        .handle
                        .as_mut())
            }
            _ => Err(VfsError::BadFd),
        }
    }

    fn ensure_fd_exists(&mut self, task_id : task::TaskId, fd : usize) -> VfsResult<()> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        if self.tables
               .get(&owner)
               .and_then(|table| table.get(fd))
               .and_then(|slot| slot.as_ref())
               .is_some()
        {
            Ok(())
        } else {
            Err(VfsError::BadFd)
        }
    }

    /// Snapshot all open descriptions so callers can flush outside the registry lock.
    pub fn all_open_handles(&self) -> Vec<SharedIoHandle> {
        self.tables
            .values()
            .flat_map(|table| {
                table.iter()
                     .flatten()
                     .cloned()
            })
            .collect()
    }

    /// 当前任务是否与其他任务共享同一 fd 表（如 `CLONE_FILES`）。
    // 本方法代码由AI完成
    pub fn is_fd_table_shared(&self, task_id : task::TaskId) -> bool {
        let owner = self.effective_owner(task_id);
        self.ref_counts
            .get(&owner)
            .copied()
            .unwrap_or(0) >
        1
    }

    /// Clone the open-file-description referenced by `fd`.
    pub fn io_handle_for_task(&mut self,
                              task_id : task::TaskId,
                              fd : usize)
                              -> VfsResult<SharedIoHandle> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        self.tables
            .get(&owner)
            .and_then(|table| table.get(fd))
            .and_then(|slot| slot.clone())
            .ok_or(VfsError::BadFd)
    }

    /// Duplicate the handle into a new independently locked fd slot.
    pub fn duplicate_handle_for_task(&mut self,
                                     task_id : task::TaskId,
                                     fd : usize)
                                     -> VfsResult<SharedIoHandle> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        self.tables
            .get(&owner)
            .and_then(|table| table.get(fd))
            .and_then(|slot| slot.as_ref())
            .ok_or(VfsError::BadFd)?
            .duplicate()
    }

    /// 按任务关闭 fd；关闭时调用句柄的 `close`。
    // 本方法代码由AI完成
    pub fn close_fd_for_task(&mut self, task_id : task::TaskId, fd : usize) -> VfsResult<()> {
        self.close_slot(task_id, fd)
    }

    /// `dup(oldfd)`：复制到 ≥ `minfd` 的最低可用 fd。
    // 本方法代码由AI完成
    pub fn install_dup_fd_for_task(&mut self,
                                   task_id : task::TaskId,
                                   minfd : usize,
                                   dup_handle : SharedIoHandle)
                                   -> VfsResult<usize> {
        if minfd >= task::nofile_rlimit_for_task(task_id) as usize {
            return Err(VfsError::TooManyOpenFiles);
        }
        self.check_nofile_before_open(task_id)?;
        self.ensure_task(task_id);
        let newfd = {
            let owner = self.effective_owner(task_id);
            let table = self.tables
                            .get_mut(&owner)
                            .expect("fd table owner");
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
        let len = self.tables
                      .get(&owner)
                      .map(Vec::len)
                      .unwrap_or(0);
        self.ensure_flags_len(task_id, len);
        self.fd_flags
            .get_mut(&owner)
            .expect("fd flags owner")[newfd] = 0;
        Ok(newfd)
    }

    /// `dup3(oldfd, newfd, cloexec)`。
    // 本方法代码由AI完成
    pub fn install_dup3_fd_for_task(&mut self,
                                    task_id : task::TaskId,
                                    newfd : usize,
                                    cloexec : bool,
                                    dup_handle : SharedIoHandle)
                                    -> VfsResult<(usize, Option<SharedIoHandle>)> {
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
        let displaced = if self.tables
                               .get(&owner)
                               .and_then(|table| table.get(newfd))
                               .and_then(|slot| slot.as_ref())
                               .is_some()
        {
            Some(self.take_fd_for_close(task_id, newfd)?)
        } else {
            None
        };
        {
            let table = self.tables
                            .get_mut(&owner)
                            .expect("fd table owner");
            while table.len() <= newfd {
                table.push(None);
            }
            table[newfd] = Some(dup_handle);
        }
        let len = self.tables
                      .get(&owner)
                      .map(Vec::len)
                      .unwrap_or(0);
        self.ensure_flags_len(task_id, len);
        self.fd_flags
            .get_mut(&owner)
            .expect("fd flags owner")[newfd] = if cloexec { FD_CLOEXEC } else { 0 };
        Ok((newfd, displaced))
    }

    /// `fcntl(F_GETFD)`。
    // 本方法代码由AI完成
    pub fn get_fd_flags(&mut self, task_id : task::TaskId, fd : usize) -> VfsResult<usize> {
        self.ensure_fd_exists(task_id, fd)?;
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let Some(flags) = self.fd_flags
                              .get(&owner)
        else {
            return Ok(0);
        };
        let v = if fd < flags.len() { flags[fd] } else { 0 };
        Ok(usize::from(v & FD_CLOEXEC))
    }

    /// `fcntl(F_SETFD)`：当前仅支持 `FD_CLOEXEC` 位。
    // 本方法代码由AI完成
    pub fn set_fd_flags(&mut self,
                        task_id : task::TaskId,
                        fd : usize,
                        val : usize)
                        -> VfsResult<()> {
        self.ensure_fd_exists(task_id, fd)?;
        let cloexec = (val & usize::from(FD_CLOEXEC)) != 0;
        self.set_fd_cloexec(task_id, fd, cloexec)
    }

    /// 将 `fd` 标记为 `O_PATH` 句柄（仅路径解析，禁止读写）。
    // 本方法代码由AI完成
    pub fn set_fd_path_only(&mut self, task_id : task::TaskId, fd : usize) -> VfsResult<()> {
        self.ensure_fd_exists(task_id, fd)?;
        self.ensure_flags_len(task_id, fd + 1);
        let owner = self.effective_owner(task_id);
        self.fd_flags
            .get_mut(&owner)
            .expect("fd flags owner")[fd] |= FD_PATH_ONLY;
        Ok(())
    }

    /// 查询 `fd` 是否为 `O_PATH` 句柄。
    // 本方法代码由AI完成
    pub fn is_fd_path_only(&mut self, task_id : task::TaskId, fd : usize) -> VfsResult<bool> {
        self.ensure_fd_exists(task_id, fd)?;
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let Some(flags) = self.fd_flags
                              .get(&owner)
        else {
            return Ok(false);
        };
        Ok(fd < flags.len() && flags[fd] & FD_PATH_ONLY != 0)
    }

    // 本方法代码由AI完成
    fn set_fd_cloexec(&mut self,
                      task_id : task::TaskId,
                      fd : usize,
                      cloexec : bool)
                      -> VfsResult<()> {
        self.ensure_flags_len(task_id, fd + 1);
        let owner = self.effective_owner(task_id);
        let slot = self.fd_flags
                       .get_mut(&owner)
                       .expect("fd flags owner");
        if cloexec {
            slot[fd] |= FD_CLOEXEC;
        } else {
            slot[fd] &= !FD_CLOEXEC;
        }
        Ok(())
    }

    // 本方法代码由AI完成
    pub fn set_fd_range_cloexec(&mut self,
                                task_id : task::TaskId,
                                first : usize,
                                last : usize,
                                cloexec : bool)
                                -> VfsResult<()> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables
                            .get(&owner)
                            .map(Vec::len)
                            .unwrap_or(0);
        if first >= table_len {
            return Ok(());
        }
        let end = last.min(table_len - 1);
        self.ensure_flags_len(task_id, end + 1);
        let flags = self.fd_flags
                        .get_mut(&owner)
                        .expect("fd flags owner");
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
    pub fn init_child_fd_table(&mut self, child : task::TaskId) { let _ = self.table_mut(child); }

    /// Snapshot a parent's fd slots and flags without calling concrete handles.
    ///
    /// The caller duplicates the returned handles after releasing the registry
    /// lock, then installs the independent child table in a second short section.
    pub fn fd_table_copy_snapshot(&mut self,
                                  parent : task::TaskId)
                                  -> (Vec<Option<SharedIoHandle>>, Vec<u8>) {
        self.ensure_task(parent);
        let parent_owner = self.effective_owner(parent);
        let parent_table = self.tables
                               .get(&parent_owner)
                               .cloned()
                               .unwrap_or_default();
        let parent_flags = self.fd_flags
                               .get(&parent_owner)
                               .cloned()
                               .unwrap_or_default();
        (parent_table, parent_flags)
    }

    /// Install an fd-table snapshot for a fork child.
    pub fn install_fd_table_copy(&mut self,
                                 child : task::TaskId,
                                 parent_table : Vec<Option<SharedIoHandle>>,
                                 parent_flags : Vec<u8>) {
        if self.owners
               .contains_key(&child)
        {
            self.drop_task_fd_table(child);
        }

        self.owners
            .insert(child, child);
        self.ref_counts
            .insert(child, 1);
        self.tables
            .insert(child, parent_table);
        self.fd_flags
            .insert(child, parent_flags);
    }

    /// thread clone 时共享父任务 fd 表。
    // 本方法代码由AI完成
    pub fn share_fd_table_from_parent(&mut self, child : task::TaskId, parent : task::TaskId) {
        self.ensure_task(parent);
        if self.owners
               .contains_key(&child)
        {
            self.drop_task_fd_table(child);
        }
        let owner = self.effective_owner(parent);
        self.owners
            .insert(child, owner);
        let count = self.ref_counts
                        .entry(owner)
                        .or_insert(0);
        *count = count.saturating_add(1);
    }

    /// `close_range(CLOSE_RANGE_UNSHARE)`：若当前任务与他人共享 fd 表，则先复制出一份独立
    /// fd 表（句柄按 fork 语义独立 duplication）；本任务本就持有唯一表时为空操作。
    // 本方法代码由AI完成
    pub fn unshare_fd_table(&mut self, task_id : task::TaskId) -> VfsResult<()> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let count = self.ref_counts
                        .get(&owner)
                        .copied()
                        .unwrap_or(1);
        if count <= 1 {
            return Ok(());
        }

        // 复制一份独立 fd 表（句柄独立 duplication，失败槽位按 fork 语义降级为关闭）。
        let parent_table = self.tables
                               .get(&owner)
                               .cloned()
                               .unwrap_or_default();
        let parent_flags = self.fd_flags
                               .get(&owner)
                               .cloned()
                               .unwrap_or_default();
        let private_table = parent_table.into_iter()
                                        .map(|slot| {
                                            slot.and_then(|handle| {
                                                    handle.duplicate()
                                                          .ok()
                                                })
                                        })
                                        .collect::<Vec<Option<SharedIoHandle>>>();

        if task_id == owner {
            // 当前任务即共享表 owner：把旧表迁移到某个兄弟共享者名下，
            // 其余共享者继续指向旧表，本任务改用自己名下的私有表。
            let sibling = self.owners
                              .iter()
                              .find(|(tid, o)| **o == owner && **tid != task_id)
                              .map(|(tid, _)| *tid)
                              .expect("shared fd table must have other sharers");
            let old_table = self.tables
                                .remove(&owner)
                                .unwrap_or_default();
            let old_flags = self.fd_flags
                                .remove(&owner)
                                .unwrap_or_default();
            let mut migrated = 0usize;
            for (tid, o) in self.owners
                                .iter_mut()
            {
                if *o == owner && *tid != task_id {
                    *o = sibling;
                    migrated += 1;
                }
            }
            self.ref_counts
                .insert(sibling, migrated);
            self.tables
                .insert(sibling, old_table);
            self.fd_flags
                .insert(sibling, old_flags);
        } else {
            // 当前任务是共享者：脱离旧表，改用私有表。
            self.owners
                .remove(&task_id);
            self.tables
                .remove(&task_id);
            self.fd_flags
                .remove(&task_id);
            if let Some(c) = self.ref_counts
                                 .get_mut(&owner)
            {
                *c = c.saturating_sub(1);
                if *c == 0 {
                    self.ref_counts
                        .remove(&owner);
                }
            }
        }

        self.owners
            .insert(task_id, task_id);
        self.ref_counts
            .insert(task_id, 1);
        self.tables
            .insert(task_id, private_table);
        self.fd_flags
            .insert(task_id, parent_flags);
        Ok(())
    }

    /// `execve` 前关闭带 `FD_CLOEXEC` 的 fd。
    // 本方法代码由AI完成
    pub fn close_cloexec_fds_for_task(&mut self, task_id : task::TaskId) {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables
                            .get(&owner)
                            .map(Vec::len)
                            .unwrap_or(0);
        let flags = self.fd_flags
                        .get(&owner)
                        .cloned()
                        .unwrap_or_default();
        for fd in (0..table_len).rev() {
            let cloexec = fd < flags.len() && (flags[fd] & FD_CLOEXEC) != 0;
            if cloexec {
                let _ = self.close_slot(task_id, fd);
            }
        }
    }

    /// 任务退出后关闭全部 fd 并清空槽位。
    // 本方法代码由AI完成
    pub fn drop_task_fd_table(&mut self, task_id : task::TaskId) {
        let Some(owner) = self.release_owner(task_id) else {
            return;
        };
        if self.ref_counts
               .get(&owner)
               .copied()
               .unwrap_or(0) ==
           0
        {
            self.close_table(owner);
        }
        if task_id != owner {
            self.tables
                .remove(&task_id);
            self.fd_flags
                .remove(&task_id);
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
    (0..character_device_count()).find(|&idx| {
                                     character_device_kind_at(idx) ==
                                     Some(CharacterDeviceKind::Serial)
                                 })
                                 .and_then(character_device_at)
}

/// 最多从物理控制台读取一个字节并送入共享 TTY。
///
/// 在执行行规程处理和投递终端信号前释放设备锁。本函数只应由唯一的低优先级控制台
/// 输入任务调用。
pub fn poll_console_input_once() -> Option<TtyControlEvent> {
    let device = default_serial_device()?;
    const POLLIN : i16 = 0x001;
    if device.lock()
             .poll_revents(POLLIN)
             .ok()? &
       POLLIN ==
       0
    {
        return None;
    }
    let mut byte = [0u8; 1];
    let read = device.lock()
                     .read(&mut byte)
                     .ok()?;
    if read == 0 {
        return None;
    }
    let (event, echo, echo_len) = tty::feed_input(byte[0]);
    if echo_len != 0 {
        console::write_raw_bytes(&echo[..echo_len]);
    }
    event
}
