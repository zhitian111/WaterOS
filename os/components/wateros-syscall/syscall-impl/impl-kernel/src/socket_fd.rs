//! Socket fd → network [`SocketRef`] 映射表。
//!
//! 因 [`VfsIoHandle`] 不支持向下转型，每个 socket fd 的共享 socket 状态在此独立维护。

//! 本模块代码由AI完成
use alloc::collections::BTreeMap;
use driver_network::SocketRef;
use spin::Mutex;

#[derive(Default)]
struct SocketFdRegistry {
    maps: BTreeMap<task::TaskId, BTreeMap<usize, SocketRef>>,
    status_flags: BTreeMap<task::TaskId, BTreeMap<usize, usize>>,
    owners: BTreeMap<task::TaskId, task::TaskId>,
    ref_counts: BTreeMap<task::TaskId, usize>,
}

impl SocketFdRegistry {
    fn ensure_owner(&mut self, task_id: task::TaskId) {
        self.maps.entry(task_id).or_insert_with(BTreeMap::new);
        self.ref_counts.entry(task_id).or_insert(1);
    }

    fn effective_owner(&self, task_id: task::TaskId) -> task::TaskId {
        self.owners.get(&task_id).copied().unwrap_or(task_id)
    }

    fn release_task(&mut self, task_id: task::TaskId) {
        let owner = self.effective_owner(task_id);
        self.owners.remove(&task_id);
        let Some(count) = self.ref_counts.get_mut(&owner) else {
            self.maps.remove(&task_id);
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.ref_counts.remove(&owner);
            self.maps.remove(&owner);
            self.status_flags.remove(&owner);
        }
    }

    fn register(&mut self, task_id: task::TaskId, fd: usize, socket: SocketRef, flags: usize) {
        self.ensure_owner(task_id);
        let owner = self.effective_owner(task_id);
        self.maps
            .entry(owner)
            .or_insert_with(BTreeMap::new)
            .insert(fd, socket);
        self.status_flags
            .entry(owner)
            .or_insert_with(BTreeMap::new)
            .insert(fd, flags);
    }

    fn lookup(&self, task_id: task::TaskId, fd: usize) -> Option<SocketRef> {
        let owner = self.effective_owner(task_id);
        self.maps.get(&owner)?.get(&fd).cloned()
    }

    fn remove(&mut self, task_id: task::TaskId, fd: usize) {
        let owner = self.effective_owner(task_id);
        if let Some(map) = self.maps.get_mut(&owner) {
            map.remove(&fd);
        }
        if let Some(flags) = self.status_flags.get_mut(&owner) {
            flags.remove(&fd);
        }
    }

    fn status_flags(&self, task_id: task::TaskId, fd: usize) -> Option<usize> {
        let owner = self.effective_owner(task_id);
        self.maps.get(&owner)?.get(&fd)?;
        Some(
            self.status_flags
                .get(&owner)
                .and_then(|flags| flags.get(&fd).copied())
                .unwrap_or(0),
        )
    }

    fn set_status_flags(
        &mut self,
        task_id: task::TaskId,
        fd: usize,
        flags: usize,
    ) -> Option<()> {
        let owner = self.effective_owner(task_id);
        self.maps.get(&owner)?.get(&fd)?;
        self.status_flags
            .entry(owner)
            .or_insert_with(BTreeMap::new)
            .insert(fd, flags);
        Some(())
    }

    fn copy_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        self.release_task(child);
        let parent_owner = self.effective_owner(parent);
        let parent_map = self.maps.get(&parent_owner).cloned().unwrap_or_default();
        let parent_flags = self
            .status_flags
            .get(&parent_owner)
            .cloned()
            .unwrap_or_default();
        self.maps.insert(child, parent_map);
        self.status_flags.insert(child, parent_flags);
        self.ref_counts.insert(child, 1);
    }

    fn share_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        self.release_task(child);
        self.ensure_owner(parent);
        let owner = self.effective_owner(parent);
        self.owners.insert(child, owner);
        *self.ref_counts.entry(owner).or_insert(0) += 1;
    }
}

static SOCKET_FD_REGISTRY: Mutex<SocketFdRegistry> = Mutex::new(SocketFdRegistry {
    maps: BTreeMap::new(),
    status_flags: BTreeMap::new(),
    owners: BTreeMap::new(),
    ref_counts: BTreeMap::new(),
});

pub(crate) fn register_with_flags(fd: usize, socket: SocketRef, flags: usize) {
    if let Some(task_id) = task::current_task_id() {
        SOCKET_FD_REGISTRY
            .lock()
            .register(task_id, fd, socket, flags);
    }
}

pub(crate) fn lookup(fd: usize) -> Option<SocketRef> {
    let task_id = task::current_task_id()?;
    SOCKET_FD_REGISTRY.lock().lookup(task_id, fd)
}

/// 查找 inet socket fd；无效 fd 返回 `EBADF`，有效非 socket 返回 `ENOTSOCK`。
pub(crate) fn lookup_or_errno(fd: usize) -> Result<SocketRef, abi::errno::ErrNo> {
    match lookup(fd) {
        Some(s) => Ok(s),
        None => {
            if vfs::fd::with_current_io(fd, |_| Ok(())).is_ok() {
                Err(abi::errno::ErrNo::ENOTSOCK)
            } else {
                Err(abi::errno::ErrNo::EBADF)
            }
        }
    }
}

pub(crate) fn remove(fd: usize) {
    if let Some(task_id) = task::current_task_id() {
        SOCKET_FD_REGISTRY.lock().remove(task_id, fd);
    }
}

pub(crate) fn status_flags(fd: usize) -> Option<usize> {
    let task_id = task::current_task_id()?;
    SOCKET_FD_REGISTRY.lock().status_flags(task_id, fd)
}

pub(crate) fn set_status_flags(fd: usize, flags: usize) -> Option<()> {
    let task_id = task::current_task_id()?;
    SOCKET_FD_REGISTRY
        .lock()
        .set_status_flags(task_id, fd, flags)
}

pub(crate) fn is_nonblocking(fd: usize) -> bool {
    const O_NONBLOCK: usize = 0o0004000;
    status_flags(fd).is_some_and(|flags| flags & O_NONBLOCK != 0)
}

pub(crate) fn copy_from_parent(child: task::TaskId, parent: task::TaskId) {
    SOCKET_FD_REGISTRY.lock().copy_from_parent(child, parent);
}

pub(crate) fn share_from_parent(child: task::TaskId, parent: task::TaskId) {
    SOCKET_FD_REGISTRY.lock().share_from_parent(child, parent);
}

pub(crate) fn drop_task(task_id: task::TaskId) {
    SOCKET_FD_REGISTRY.lock().release_task(task_id);
}
