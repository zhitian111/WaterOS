//! 以 [`task::TaskId`] 为 key 的 per-task fd 表。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::sync::Arc;
use alloc::vec::Vec;

use spin::Mutex;

use api_v0::{
    VfsError, VfsFdSession, VfsIoHandle, VfsPreparedRead, VfsResourceKind, VfsResult,
    VFS_FIRST_DYNAMIC_FD, VFS_STDERR_FD, VFS_STDIN_FD, VFS_STDOUT_FD,
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

/// 仅由临时 I/O 租约共享的稳定 fd 槽句柄。
#[derive(Clone)]
pub struct SharedIoHandle {
    inner : Arc<Mutex<OpenFileDescription>>,
    snapshot : Arc<Mutex<Option<OpenFileDescription>>>,
    /// PTY 身份对一个打开文件描述不可变。安装时缓存该身份，使 close-on-exec
    /// 无需等待活动 I/O 租约即可判断需要向哪个终端发送挂断通知。
    terminal_id : Option<tty::TerminalId>,
    resource_kind : VfsResourceKind,
}

impl SharedIoHandle {
    pub fn new(handle : Box<dyn VfsIoHandle>) -> Self {
        let resource_kind = handle.resource_kind();
        let terminal_id =
            crate::pty_endpoint_for_handle(handle.as_ref()).map(|endpoint| endpoint.id());
        let snapshot = handle.duplicate()
                             .ok()
                             .map(OpenFileDescription::new);
        Self { inner : Arc::new(Mutex::new(OpenFileDescription::new(handle))),
               snapshot : Arc::new(Mutex::new(snapshot)),
               terminal_id,
               resource_kind }
    }

    /// 返回不可变 PTY 身份，不获取 I/O 租约。
    pub fn terminal_id(&self) -> Option<tty::TerminalId> { self.terminal_id }

    pub fn resource_kind(&self) -> VfsResourceKind { self.resource_kind }

    pub fn with_io<R>(&self,
                      f : impl FnOnce(&mut (dyn VfsIoHandle + '_)) -> VfsResult<R>)
                      -> VfsResult<R> {
        let mut inner = self.inner.lock();
        f(inner.handle
               .as_mut())
    }

    /// 只短暂持有 fd 槽锁，捕获一次 prepared read。
    pub fn prepare_read(&self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        let mut inner = self.inner.lock();
        inner.handle
             .prepare_read(max_len)
    }

    /// 创建独立 fd 槽句柄；实时句柄阻塞时使用 I/O 前保存的快照。
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

    /// 最后一个引用立即关闭；若 I/O 租约仍存在，则由其析构延迟关闭。
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

#[derive(Clone)]
struct FdSlot {
    handle : SharedIoHandle,
    flags : u8,
    resource_kind : VfsResourceKind,
    terminal_id : Option<tty::TerminalId>,
}

impl FdSlot {
    fn new(handle : SharedIoHandle, flags : u8) -> Self {
        Self { resource_kind : handle.resource_kind(),
               terminal_id : handle.terminal_id(),
               handle,
               flags }
    }

    fn snapshot(&self) -> FdSlotSnapshot {
        FdSlotSnapshot { handle : self.handle.clone(),
                         flags : self.flags,
                         resource_kind : self.resource_kind,
                         terminal_id : self.terminal_id }
    }

    fn duplicate(&self) -> VfsResult<Self> {
        Ok(Self::new(self.handle.duplicate()?, self.flags))
    }
}

/// 一次 FD registry 查询返回的稳定 slot 分类与句柄快照。
#[derive(Clone)]
pub struct FdSlotSnapshot {
    pub handle : SharedIoHandle,
    pub flags : u8,
    pub resource_kind : VfsResourceKind,
    pub terminal_id : Option<tty::TerminalId>,
}

/// 普通 fork 子进程共享的不可变 fd 表镜像，直到任一方修改描述符表。
/// 各打开文件描述按 fork(2) 语义共享，而下方 `Arc::make_mut` 保证描述符表修改时彼此独立。
pub struct ForkFdTableSnapshot {
    table : Arc<Vec<Option<FdSlot>>>,
}

impl FdSlotSnapshot {
    pub fn duplicate(&self) -> VfsResult<Self> {
        let handle = self.handle.duplicate()?;
        Ok(Self { handle,
                  flags : self.flags,
                  resource_kind : self.resource_kind,
                  terminal_id : self.terminal_id })
    }

    fn into_slot(self) -> FdSlot {
        FdSlot { handle : self.handle,
                 flags : self.flags,
                 resource_kind : self.resource_kind,
                 terminal_id : self.terminal_id }
    }
}

/// 全局 per-task fd 注册表。
// 本结构代码由AI完成
pub struct PerTaskFdRegistry {
    tables : BTreeMap<task::TaskId, Arc<Vec<Option<FdSlot>>>>,
    owners : BTreeMap<task::TaskId, task::TaskId>,
    ref_counts : BTreeMap<task::TaskId, usize>,
    open_counts : BTreeMap<task::TaskId, usize>,
    free_fds : BTreeMap<task::TaskId, BTreeSet<usize>>,
}

impl PerTaskFdRegistry {
    pub const fn new() -> Self {
        Self { tables : BTreeMap::new(),
               owners : BTreeMap::new(),
               ref_counts : BTreeMap::new(),
               open_counts : BTreeMap::new(),
               free_fds : BTreeMap::new() }
    }

    // 本方法代码由AI完成
    fn ensure_task(&mut self, task_id : task::TaskId) {
        if self.initialized_owner(task_id)
               .is_some()
        {
            return;
        }
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
        let table = Arc::make_mut(self.tables
                                      .entry(owner)
                                      .or_insert_with(|| Arc::new(Vec::new())));
        if table.len() < VFS_FIRST_DYNAMIC_FD {
            table.resize_with(VFS_FIRST_DYNAMIC_FD, || None);
            table[VFS_STDIN_FD] = Some(FdSlot::new(SharedIoHandle::new(default_stdin_handle()), 0));
            table[VFS_STDOUT_FD] =
                Some(FdSlot::new(SharedIoHandle::new(default_stdout_handle()), 0));
            table[VFS_STDERR_FD] =
                Some(FdSlot::new(SharedIoHandle::new(default_stdout_handle()), 0));
            self.open_counts
                .insert(owner, VFS_FIRST_DYNAMIC_FD);
            self.free_fds
                .entry(owner)
                .or_default()
                .clear();
        }
    }

    /// 返回已绑定、fd 表已初始化且 refcount 已登记的 owner。
    ///
    /// 这是 syscall 热路径的常见状态；调用方拿到 `Some(owner)` 后可直接访问
    /// `tables`，不再重复执行 BTreeMap entry/insert。
    fn initialized_owner(&self, task_id : task::TaskId) -> Option<task::TaskId> {
        let owner = self.owners
                        .get(&task_id)
                        .copied()?;
        let table = self.tables
                        .get(&owner)?;
        (table.len() >= VFS_FIRST_DYNAMIC_FD &&
         self.ref_counts
             .contains_key(&owner)).then_some(owner)
    }

    // 本方法代码由AI完成
    fn effective_owner(&self, task_id : task::TaskId) -> task::TaskId {
        self.owners
            .get(&task_id)
            .copied()
            .unwrap_or(task_id)
    }

    // 本方法代码由AI完成
    fn table_mut(&mut self, task_id : task::TaskId) -> &mut Vec<Option<FdSlot>> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        Arc::make_mut(self.tables
                          .get_mut(&owner)
                          .expect("fd table owner"))
    }

    fn resize_table_with_holes(&mut self, owner : task::TaskId, new_len : usize) {
        let table = Arc::make_mut(self.tables
                                      .get_mut(&owner)
                                      .expect("fd table owner"));
        let old_len = table.len();
        if old_len < new_len {
            table.resize_with(new_len, || None);
            let free = self.free_fds
                           .entry(owner)
                           .or_default();
            for fd in old_len..new_len {
                free.insert(fd);
            }
        }
    }

    fn mark_fd_open(&mut self, owner : task::TaskId, fd : usize) {
        let count = self.open_counts
                        .entry(owner)
                        .or_insert(0);
        *count = count.saturating_add(1);
        self.free_fds
            .entry(owner)
            .or_default()
            .remove(&fd);
    }

    fn mark_fd_closed(&mut self, owner : task::TaskId, fd : usize) {
        let count = self.open_counts
                        .entry(owner)
                        .or_insert(0);
        *count = count.saturating_sub(1);
        self.free_fds
            .entry(owner)
            .or_default()
            .insert(fd);
    }

    fn rebuild_table_indexes(&mut self, owner : task::TaskId) {
        let Some(table) = self.tables.get(&owner) else {
            self.open_counts.remove(&owner);
            self.free_fds.remove(&owner);
            return;
        };
        let open_count = table.iter().filter(|slot| slot.is_some()).count();
        let free = table.iter()
                        .enumerate()
                        .filter_map(|(fd, slot)| slot.is_none().then_some(fd))
                        .collect();
        self.open_counts.insert(owner, open_count);
        self.free_fds.insert(owner, free);
    }

    fn alloc_slot_for_owner(&mut self, owner : task::TaskId, handle : SharedIoHandle) -> usize {
        self.alloc_slot_for_owner_from(owner, 0, handle)
    }

    fn alloc_slot_for_owner_from(&mut self,
                                 owner : task::TaskId,
                                 minfd : usize,
                                 handle : SharedIoHandle)
                                 -> usize {
        let candidate = {
            let free = self.free_fds
                           .entry(owner)
                           .or_default();
            free.range(minfd..)
                .next()
                .copied()
        };
        let fd = if let Some(fd) = candidate {
            self.free_fds
                .get_mut(&owner)
                .expect("fd free set owner")
                .remove(&fd);
            let table = Arc::make_mut(self.tables
                                          .get_mut(&owner)
                                          .expect("fd table owner"));
            table[fd] = Some(FdSlot::new(handle, 0));
            fd
        } else {
            let table = Arc::make_mut(self.tables
                                          .get_mut(&owner)
                                          .expect("fd table owner"));
            let old_len = table.len();
            if old_len < minfd {
                table.resize_with(minfd, || None);
                let free = self.free_fds
                               .entry(owner)
                               .or_default();
                for fd in old_len..minfd {
                    free.insert(fd);
                }
            }
            let fd = table.len();
            table.push(Some(FdSlot::new(handle, 0)));
            fd
        };
        self.mark_fd_open(owner, fd);
        fd
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
        if let Some(table) = self.tables.remove(&owner) {
            // 从未修改 fd 表的 fork 子进程只需减少一次 Arc 引用即可释放共享表；
            // 只有最后一个所有者需要遍历并关闭所有打开文件描述。
            if let Ok(mut table) = Arc::try_unwrap(table) {
                for slot in table.iter_mut() {
                    if let Some(slot) = slot.take() {
                        handles.push(slot.handle);
                    }
                }
            }
        }
        self.open_counts
            .remove(&owner);
        self.free_fds
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
        let handle = Arc::make_mut(self.tables
                                       .get_mut(&owner)
                                       .ok_or(VfsError::BadFd)?)
                         .get_mut(fd)
                         .ok_or(VfsError::BadFd)?
                         .take()
                         .ok_or(VfsError::BadFd)?;
        self.mark_fd_closed(owner, fd);
        Ok(handle.handle)
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
                            .map(|table| table.len())
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
            let handle = Arc::make_mut(self.tables
                                           .get_mut(&owner)
                                           .expect("fd table owner"))
                             .get_mut(fd)
                             .expect("fd in range")
                             .take()
                             .expect("checked Some");
            self.mark_fd_closed(owner, fd);
            handles.push((fd, handle.handle));
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
                            .map(|table| table.len())
                            .unwrap_or(0);
        let mut handles = Vec::new();
        for fd in (0..table_len).rev() {
            let cloexec = self.tables
                              .get(&owner)
                              .and_then(|table| table.get(fd))
                              .and_then(Option::as_ref)
                              .is_some_and(|slot| slot.flags & FD_CLOEXEC != 0);
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
        self.open_counts
            .get(&owner)
            .copied()
            .unwrap_or(0)
    }

    /// 调试面板用的全局 fd 注册表摘要。
    ///
    /// `task_bindings` 包含共享同一张 fd 表的任务；`table_count` 是实际独立 fd 表数。
    /// 调用方必须已经持有注册表锁。
    pub fn debug_counts(&self) -> (usize, usize, usize) {
        let open_fd_count = self.open_counts
                                .values()
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
                let inner = Arc::get_mut(&mut h.handle.inner).ok_or(VfsError::Busy)?;
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
        self.alloc_fd_for_task(task_id, handle)
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
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        Ok(self.alloc_slot_for_owner(owner, SharedIoHandle::new(handle)))
    }

    /// 按任务与 fd 号取可变句柄。
    // 本方法代码由AI完成
    pub fn get_io_for_task(&mut self,
                           task_id : task::TaskId,
                           fd : usize)
                           -> VfsResult<&mut (dyn VfsIoHandle + '_)> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table = Arc::make_mut(self.tables
                                      .get_mut(&owner)
                                      .expect("fd table owner"));
        match table.get_mut(fd) {
            Some(Some(h)) => {
                let inner = Arc::get_mut(&mut h.handle.inner).ok_or(VfsError::Busy)?;
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

    /// 快照所有打开描述，使调用方能在 registry 锁外执行刷新。
    pub fn all_open_handles(&self) -> Vec<SharedIoHandle> {
        self.tables
            .values()
            .flat_map(|table| {
                table.iter()
                     .flatten()
                     .map(|slot| slot.handle.clone())
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

    /// 克隆 `fd` 引用的打开文件描述。
    pub fn io_handle_for_task(&mut self,
                              task_id : task::TaskId,
                              fd : usize)
                              -> VfsResult<SharedIoHandle> {
        if let Some(owner) = self.initialized_owner(task_id) {
            return self.tables
                       .get(&owner)
                       .and_then(|table| table.get(fd))
                       .and_then(Option::as_ref)
                       .map(|slot| slot.handle.clone())
                       .ok_or(VfsError::BadFd);
        }
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        self.tables
            .get(&owner)
            .and_then(|table| table.get(fd))
            .and_then(Option::as_ref)
            .map(|slot| slot.handle.clone())
            .ok_or(VfsError::BadFd)
    }

    /// 在一次 registry 查找中快照句柄、描述符标志和不可变资源分类。
    pub fn fd_slot_for_task(&mut self,
                            task_id : task::TaskId,
                            fd : usize)
                            -> VfsResult<FdSlotSnapshot> {
        if let Some(owner) = self.initialized_owner(task_id) {
            return self.tables
                       .get(&owner)
                       .and_then(|table| table.get(fd))
                       .and_then(Option::as_ref)
                       .map(FdSlot::snapshot)
                       .ok_or(VfsError::BadFd);
        }
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        self.tables
            .get(&owner)
            .and_then(|table| table.get(fd))
            .and_then(Option::as_ref)
            .map(FdSlot::snapshot)
            .ok_or(VfsError::BadFd)
    }

    /// 将句柄复制到新的、独立加锁的 fd 槽位。
    pub fn duplicate_handle_for_task(&mut self,
                                     task_id : task::TaskId,
                                     fd : usize)
                                     -> VfsResult<SharedIoHandle> {
        if let Some(owner) = self.initialized_owner(task_id) {
            return self.tables
                       .get(&owner)
                       .and_then(|table| table.get(fd))
                       .and_then(|slot| slot.as_ref())
                       .ok_or(VfsError::BadFd)?
                       .handle
                       .duplicate();
        }
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        self.tables
            .get(&owner)
            .and_then(|table| table.get(fd))
            .and_then(|slot| slot.as_ref())
            .ok_or(VfsError::BadFd)?
            .handle
            .duplicate()
    }

    /// 复制指定任务的 fd 为一个尚未安装进任何 fd 表的独立句柄。
    ///
    /// `pidfd_getfd` 使用该入口跨进程取得打开文件描述；调用方随后必须把返回
    /// 句柄安装到自己的 fd 表。这里不长期持有 registry 或 fd 槽锁。
    pub fn duplicate_io_for_task(&mut self,
                                 task_id : task::TaskId,
                                 fd : usize)
                                 -> VfsResult<Box<dyn VfsIoHandle>> {
        let handle = self.io_handle_for_task(task_id, fd)?;
        handle.with_io(|io| io.duplicate())
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
        let owner = self.effective_owner(task_id);
        let newfd = self.alloc_slot_for_owner_from(owner, minfd, dup_handle);
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
        self.resize_table_with_holes(owner, newfd + 1);
        Arc::make_mut(self.tables
                          .get_mut(&owner)
                          .expect("fd table owner"))[newfd] =
            Some(FdSlot::new(dup_handle, if cloexec { FD_CLOEXEC } else { 0 }));
        self.mark_fd_open(owner, newfd);
        Ok((newfd, displaced))
    }

    /// `fcntl(F_GETFD)`。
    // 本方法代码由AI完成
    pub fn get_fd_flags(&mut self, task_id : task::TaskId, fd : usize) -> VfsResult<usize> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let flags = self.tables
                        .get(&owner)
                        .and_then(|table| table.get(fd))
                        .and_then(Option::as_ref)
                        .ok_or(VfsError::BadFd)?
                        .flags;
        Ok(usize::from(flags & FD_CLOEXEC))
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
        let owner = self.effective_owner(task_id);
        Arc::make_mut(self.tables
                          .get_mut(&owner)
                          .expect("fd table owner"))
            .get_mut(fd)
            .and_then(Option::as_mut)
            .expect("checked fd slot")
            .flags |= FD_PATH_ONLY;
        Ok(())
    }

    /// 查询 `fd` 是否为 `O_PATH` 句柄。
    // 本方法代码由AI完成
    pub fn is_fd_path_only(&mut self, task_id : task::TaskId, fd : usize) -> VfsResult<bool> {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let slot = self.tables
                       .get(&owner)
                       .and_then(|table| table.get(fd))
                       .and_then(Option::as_ref)
                       .ok_or(VfsError::BadFd)?;
        Ok(slot.flags & FD_PATH_ONLY != 0)
    }

    // 本方法代码由AI完成
    fn set_fd_cloexec(&mut self,
                      task_id : task::TaskId,
                      fd : usize,
                      cloexec : bool)
                      -> VfsResult<()> {
        self.ensure_fd_exists(task_id, fd)?;
        let owner = self.effective_owner(task_id);
        let slot = Arc::make_mut(self.tables
                                     .get_mut(&owner)
                                     .expect("fd table owner"))
                       .get_mut(fd)
                       .and_then(Option::as_mut)
                       .expect("checked fd slot");
        if cloexec {
            slot.flags |= FD_CLOEXEC;
        } else {
            slot.flags &= !FD_CLOEXEC;
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
                            .map(|table| table.len())
                            .unwrap_or(0);
        if first >= table_len {
            return Ok(());
        }
        let end = last.min(table_len - 1);
        let table = Arc::make_mut(self.tables
                                      .get_mut(&owner)
                                      .expect("fd table owner"));
        for fd in first..=end {
            let Some(slot) = table[fd].as_mut() else {
                continue;
            };
            if cloexec {
                slot.flags |= FD_CLOEXEC;
            } else {
                slot.flags &= !FD_CLOEXEC;
            }
        }
        Ok(())
    }

    /// fork 时初始化子任务 fd 表：仅默认 stdin/stdout/stderr（spawn 路径）。
    // 本方法代码由AI完成
    pub fn init_child_fd_table(&mut self, child : task::TaskId) { let _ = self.table_mut(child); }

    /// 快照父任务的 fd 槽位和标志，不调用具体句柄。
    ///
    /// 调用方在释放 registry 锁后复制返回句柄，再在第二个短临界区安装独立的子表。
    pub fn fd_table_copy_snapshot(&mut self,
                                  parent : task::TaskId)
                                  -> Vec<Option<FdSlotSnapshot>> {
        self.ensure_task(parent);
        let parent_owner = self.effective_owner(parent);
        self.tables
            .get(&parent_owner)
            .map(|table| table.iter()
                               .map(|slot| slot.as_ref().map(FdSlot::snapshot))
                               .collect())
            .unwrap_or_default()
    }

    /// O(1) fork 快照。描述符表存储和打开文件描述保持共享，直到描述符操作修改任一进程的表。
    pub fn fd_table_fork_snapshot(&mut self, parent : task::TaskId) -> ForkFdTableSnapshot {
        self.ensure_task(parent);
        let parent_owner = self.effective_owner(parent);
        ForkFdTableSnapshot { table : self.tables
                                           .get(&parent_owner)
                                           .cloned()
                                           .unwrap_or_default() }
    }

    pub fn install_fd_table_fork_snapshot(&mut self,
                                          child : task::TaskId,
                                          snapshot : ForkFdTableSnapshot) {
        if self.owners.contains_key(&child) {
            self.drop_task_fd_table(child);
        }
        self.owners.insert(child, child);
        self.ref_counts.insert(child, 1);
        self.tables.insert(child, snapshot.table);
        self.rebuild_table_indexes(child);
    }

    /// 为 fork 子进程安装 fd 表快照。
    pub fn install_fd_table_copy(&mut self,
                                 child : task::TaskId,
                                 parent_table : Vec<Option<FdSlotSnapshot>>) {
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
            .insert(child,
                    Arc::new(parent_table.into_iter()
                                         .map(|slot| slot.map(FdSlotSnapshot::into_slot))
                                         .collect()));
        self.rebuild_table_indexes(child);
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
        let private_table = parent_table.iter()
                                        .cloned()
                                        .map(|slot| {
                                            slot.and_then(|slot| slot.duplicate().ok())
                                        })
                                        .collect::<Vec<Option<FdSlot>>>();

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
            self.open_counts.remove(&owner);
            self.free_fds.remove(&owner);
            self.rebuild_table_indexes(sibling);
        } else {
            // 当前任务是共享者：脱离旧表，改用私有表。
            self.owners
                .remove(&task_id);
            self.tables
                .remove(&task_id);
            self.open_counts.remove(&task_id);
            self.free_fds.remove(&task_id);
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
            .insert(task_id, Arc::new(private_table));
        self.rebuild_table_indexes(task_id);
        Ok(())
    }

    /// `execve` 前关闭带 `FD_CLOEXEC` 的 fd。
    // 本方法代码由AI完成
    pub fn close_cloexec_fds_for_task(&mut self, task_id : task::TaskId) {
        self.ensure_task(task_id);
        let owner = self.effective_owner(task_id);
        let table_len = self.tables
                            .get(&owner)
                            .map(|table| table.len())
                            .unwrap_or(0);
        for fd in (0..table_len).rev() {
            let cloexec = self.tables
                              .get(&owner)
                              .and_then(|table| table.get(fd))
                              .and_then(Option::as_ref)
                              .is_some_and(|slot| slot.flags & FD_CLOEXEC != 0);
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
            self.open_counts
                .remove(&task_id);
            self.free_fds
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

/// 为 `/dev/tty` 创建一个指向系统控制台的双向字符设备句柄。
pub(crate) fn open_console_tty(accmode : u32) -> Option<Box<dyn VfsIoHandle>> {
    default_serial_device().map(|device| {
                               Box::new(CharDevHandle::from_devfs_path(device, "/dev/tty", accmode))
                               as Box<dyn VfsIoHandle>
                           })
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
