//! epoll fd → [`EpollInstance`] 映射表（[`VfsIoHandle`] 无法向下转型）。

//! 本模块代码由AI完成
extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;
use vfs::api::{VfsIoHandle, VfsMetadata, VfsNodeType, VfsResult};

use crate::poll_engine::{poll_revents_fd, POLLIN, POLLNVAL};

static NEXT_EPOLL_INODE : AtomicU64 = AtomicU64::new(1);

/// 单个 interest 条目。
#[derive(Debug, Clone)]
// 本结构代码由AI完成
pub(crate) struct EpollInterest {
    pub events : u32,
    pub data : u64,
}

/// epoll 实例状态。
#[derive(Debug, Clone)]
// 本结构代码由AI完成
pub(crate) struct EpollInstance {
    pub interests : BTreeMap<usize, EpollInterest>,
    inode : u64,
}

impl EpollInstance {
    pub(crate) fn new() -> Self {
        Self { interests : BTreeMap::new(),
               inode : NEXT_EPOLL_INODE.fetch_add(1, Ordering::Relaxed) }
    }
}

/// epoll 匿名 fd 句柄。
pub(crate) struct EpollHandle {
    inner : Arc<Mutex<EpollInstance>>,
}

impl EpollHandle {
    pub(crate) fn new_pair() -> (Self, Arc<Mutex<EpollInstance>>) {
        let inner = Arc::new(Mutex::new(EpollInstance::new()));
        (Self { inner : inner.clone() }, inner)
    }
}

impl VfsIoHandle for EpollHandle {
    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
        if events & POLLIN == 0 {
            return Ok(0);
        }
        let guard = self.inner.lock();
        for (&fd, interest) in &guard.interests {
            let poll_events = epoll_to_poll_events(interest.events);
            let revents = poll_revents_fd(fd, poll_events);
            if revents != 0 && revents & POLLNVAL == 0 {
                return Ok(POLLIN);
            }
        }
        Ok(0)
    }

    fn duplicate(&self) -> VfsResult<alloc::boxed::Box<dyn VfsIoHandle>> {
        Ok(alloc::boxed::Box::new(Self { inner : self.inner.clone() }))
    }

    fn close(&mut self) -> VfsResult<()> {
        self.inner
            .lock()
            .interests
            .clear();
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        let inode = self.inner
                        .lock()
                        .inode;
        Ok(VfsMetadata { node_type : VfsNodeType::Special,
                         size : 0,
                         mode : 0o0600,
                         device_major : 0,
                         device_minor : 0,
                         inode,
                         mount_id : 0,
                         nlink : 1,
                         uid : 0,
                         gid : 0 })
    }
}

#[derive(Default)]
struct EpollFdRegistry {
    maps : BTreeMap<task::TaskId, BTreeMap<usize, Arc<Mutex<EpollInstance>>>>,
    owners : BTreeMap<task::TaskId, task::TaskId>,
    ref_counts : BTreeMap<task::TaskId, usize>,
}

impl EpollFdRegistry {
    fn ensure_owner(&mut self, task_id : task::TaskId) {
        self.maps
            .entry(task_id)
            .or_insert_with(BTreeMap::new);
        self.ref_counts
            .entry(task_id)
            .or_insert(1);
    }

    fn effective_owner(&self, task_id : task::TaskId) -> task::TaskId {
        self.owners
            .get(&task_id)
            .copied()
            .unwrap_or(task_id)
    }

    fn release_task(&mut self, task_id : task::TaskId) {
        let owner = self.effective_owner(task_id);
        self.owners
            .remove(&task_id);
        let Some(count) = self.ref_counts
                              .get_mut(&owner)
        else {
            self.maps
                .remove(&task_id);
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.ref_counts
                .remove(&owner);
            self.maps
                .remove(&owner);
        }
    }

    fn register(&mut self,
                task_id : task::TaskId,
                fd : usize,
                instance : Arc<Mutex<EpollInstance>>) {
        self.ensure_owner(task_id);
        let owner = self.effective_owner(task_id);
        self.maps
            .entry(owner)
            .or_insert_with(BTreeMap::new)
            .insert(fd, instance);
    }

    fn lookup(&self, task_id : task::TaskId, fd : usize) -> Option<Arc<Mutex<EpollInstance>>> {
        let owner = self.effective_owner(task_id);
        self.maps
            .get(&owner)?
            .get(&fd)
            .cloned()
    }

    fn remove(&mut self, task_id : task::TaskId, fd : usize) {
        let owner = self.effective_owner(task_id);
        if let Some(map) = self.maps
                               .get_mut(&owner)
        {
            map.remove(&fd);
        }
    }

    fn copy_from_parent(&mut self, child : task::TaskId, parent : task::TaskId) {
        self.release_task(child);
        let parent_owner = self.effective_owner(parent);
        let parent_map = self.maps
                             .get(&parent_owner)
                             .cloned()
                             .unwrap_or_default();
        self.maps
            .insert(child, parent_map);
        self.ref_counts
            .insert(child, 1);
    }

    fn share_from_parent(&mut self, child : task::TaskId, parent : task::TaskId) {
        self.release_task(child);
        self.ensure_owner(parent);
        let owner = self.effective_owner(parent);
        self.owners
            .insert(child, owner);
        *self.ref_counts
             .entry(owner)
             .or_insert(0) += 1;
    }
}

static EPOLL_FD_REGISTRY : Mutex<EpollFdRegistry> =
    Mutex::new(EpollFdRegistry { maps : BTreeMap::new(),
                                 owners : BTreeMap::new(),
                                 ref_counts : BTreeMap::new() });

pub(crate) fn register(fd : usize, instance : Arc<Mutex<EpollInstance>>) {
    if let Some(task_id) = task::current_task_id() {
        EPOLL_FD_REGISTRY.lock()
                         .register(task_id, fd, instance);
    }
}

pub(crate) fn lookup(fd : usize) -> Option<Arc<Mutex<EpollInstance>>> {
    let task_id = task::current_task_id()?;
    EPOLL_FD_REGISTRY.lock()
                     .lookup(task_id, fd)
}

pub(crate) fn is_epoll_fd(fd : usize) -> bool { lookup(fd).is_some() }

pub(crate) fn remove(fd : usize) {
    if let Some(task_id) = task::current_task_id() {
        EPOLL_FD_REGISTRY.lock()
                         .remove(task_id, fd);
    }
}

pub(crate) fn copy_from_parent(child : task::TaskId, parent : task::TaskId) {
    EPOLL_FD_REGISTRY.lock()
                     .copy_from_parent(child, parent);
}

pub(crate) fn share_from_parent(child : task::TaskId, parent : task::TaskId) {
    EPOLL_FD_REGISTRY.lock()
                     .share_from_parent(child, parent);
}

pub(crate) fn drop_task(task_id : task::TaskId) {
    EPOLL_FD_REGISTRY.lock()
                     .release_task(task_id);
}

pub(crate) const EPOLLIN : u32 = 0x001;
pub(crate) const EPOLLOUT : u32 = 0x004;
pub(crate) const EPOLLPRI : u32 = 0x002;
pub(crate) const EPOLLERR : u32 = 0x008;
pub(crate) const EPOLLHUP : u32 = 0x010;
pub(crate) const EPOLLRDHUP : u32 = 0x2000;
pub(crate) const EPOLLET : u32 = 0x8000_0000;
pub(crate) const EPOLLONESHOT : u32 = 0x4000_0000;
pub(crate) const EPOLL_CLOEXEC : usize = 0o2000000;

pub(crate) const EPOLL_CTL_ADD : usize = 1;
pub(crate) const EPOLL_CTL_DEL : usize = 2;
pub(crate) const EPOLL_CTL_MOD : usize = 3;

pub(crate) const EPOLL_VALID_EVENTS : u32 =
    EPOLLIN | EPOLLOUT | EPOLLPRI | EPOLLERR | EPOLLHUP | EPOLLRDHUP | EPOLLET | EPOLLONESHOT;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct EpollEvent {
    pub events : u32,
    pub data : u64,
}

// 本方法代码由AI完成
pub(crate) fn epoll_to_poll_events(events : u32) -> i16 {
    let mut poll = 0i16;
    if events & EPOLLIN != 0 {
        poll |= POLLIN;
    }
    if events & EPOLLOUT != 0 {
        poll |= crate::poll_engine::POLLOUT;
    }
    if events & EPOLLPRI != 0 {
        poll |= crate::poll_engine::POLLPRI;
    }
    if events & EPOLLERR != 0 {
        poll |= crate::poll_engine::POLLERR;
    }
    if events & EPOLLHUP != 0 {
        poll |= crate::poll_engine::POLLHUP;
    }
    poll
}

// 本方法代码由AI完成
pub(crate) fn poll_to_epoll_events(revents : i16) -> u32 {
    let mut events = 0u32;
    if revents & POLLIN != 0 {
        events |= EPOLLIN;
    }
    if revents & crate::poll_engine::POLLOUT != 0 {
        events |= EPOLLOUT;
    }
    if revents & crate::poll_engine::POLLPRI != 0 {
        events |= EPOLLPRI;
    }
    if revents & crate::poll_engine::POLLERR != 0 {
        events |= EPOLLERR;
    }
    if revents & crate::poll_engine::POLLHUP != 0 {
        events |= EPOLLHUP;
    }
    events
}
