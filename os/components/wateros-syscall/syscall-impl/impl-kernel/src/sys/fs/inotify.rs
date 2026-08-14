//! Linux `inotify_init1/add_watch/rm_watch` 的内核实现。
//!
//! 首版监听对象以规范化绝对路径标识。文件系统 syscall 在修改成功、且已释放
//! VFS/fd 锁后调用本模块的 `notify_*`，事件队列因此不会反向嵌套文件系统锁。

extern crate alloc;

use alloc::{
    boxed::Box,
    collections::{BTreeMap, VecDeque},
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicU32, Ordering};

use api_v0::{ErrNo, SyscallArgs, UserRet};
use spin::Mutex;
use vfs::{
    active_impl,
    api::{
        FinalSymlink, SingleRootReadView, VfsCopyProgress, VfsError, VfsIoHandle, VfsMetadata,
        VfsNodeType, VfsPreparedRead, VfsReadFinish, VfsReadLease, VfsResult,
    },
    fd,
};

use super::path_at::{resolve_path_at, resolve_symlinks, AT_FDCWD};
use crate::{
    user_copy::copy_user_path_cstr,
    vfs_util::vfs_error_to_errno,
};

pub(crate) const IN_ACCESS : u32 = 0x0000_0001;
pub(crate) const IN_MODIFY : u32 = 0x0000_0002;
pub(crate) const IN_ATTRIB : u32 = 0x0000_0004;
pub(crate) const IN_CLOSE_WRITE : u32 = 0x0000_0008;
pub(crate) const IN_CLOSE_NOWRITE : u32 = 0x0000_0010;
pub(crate) const IN_OPEN : u32 = 0x0000_0020;
pub(crate) const IN_MOVED_FROM : u32 = 0x0000_0040;
pub(crate) const IN_MOVED_TO : u32 = 0x0000_0080;
pub(crate) const IN_CREATE : u32 = 0x0000_0100;
pub(crate) const IN_DELETE : u32 = 0x0000_0200;
pub(crate) const IN_DELETE_SELF : u32 = 0x0000_0400;
pub(crate) const IN_MOVE_SELF : u32 = 0x0000_0800;
const IN_Q_OVERFLOW : u32 = 0x0000_4000;
const IN_IGNORED : u32 = 0x0000_8000;
const IN_ONLYDIR : u32 = 0x0100_0000;
const IN_DONT_FOLLOW : u32 = 0x0200_0000;
const IN_EXCL_UNLINK : u32 = 0x0400_0000;
const IN_MASK_CREATE : u32 = 0x1000_0000;
const IN_MASK_ADD : u32 = 0x2000_0000;
const IN_ISDIR : u32 = 0x4000_0000;
const IN_ONESHOT : u32 = 0x8000_0000;
const IN_ALL_EVENTS : u32 = 0x0000_0fff;

const IN_NONBLOCK : usize = 0o0004000;
const IN_CLOEXEC : usize = 0o2000000;
const FD_CLOEXEC : usize = 1;
const POLLIN : i16 = 0x001;
const EVENT_HEADER_LEN : usize = 16;
const MAX_QUEUED_EVENTS : usize = 4096;

const WATCH_MODIFIERS : u32 = IN_ONLYDIR | IN_DONT_FOLLOW | IN_EXCL_UNLINK | IN_MASK_CREATE |
                              IN_MASK_ADD | IN_ONESHOT;
const VALID_WATCH_MASK : u32 = IN_ALL_EVENTS | WATCH_MODIFIERS;

static INSTANCES : Mutex<Vec<Weak<InotifyState>>> = Mutex::new(Vec::new());
static NEXT_RENAME_COOKIE : AtomicU32 = AtomicU32::new(1);

#[derive(Clone)]
struct Watch {
    path : String,
    mask : u32,
}

struct QueuedEvent {
    bytes : Vec<u8>,
}

struct InotifyInner {
    watches : BTreeMap<i32, Watch>,
    queue : VecDeque<QueuedEvent>,
    next_wd : i32,
    nonblocking : bool,
    overflow_queued : bool,
}

struct InotifyState {
    inner : Mutex<InotifyInner>,
    wait : task::wait_queue::WaitQueue,
}

impl InotifyState {
    fn new(nonblocking : bool) -> Arc<Self> {
        let state = Arc::new(Self {
            inner : Mutex::new(InotifyInner { watches : BTreeMap::new(),
                                             queue : VecDeque::new(),
                                             next_wd : 1,
                                             nonblocking,
                                             overflow_queued : false }),
            wait : task::wait_queue::WaitQueue::new_named("inotify"),
        });
        let mut instances = INSTANCES.lock();
        instances.retain(|entry| entry.strong_count() != 0);
        instances.push(Arc::downgrade(&state));
        state
    }

    fn has_events(&self) -> bool { !self.inner.lock().queue.is_empty() }

    fn add_watch(&self, path : String, mask : u32) -> Result<i32, ErrNo> {
        let mut inner = self.inner.lock();
        if let Some((&wd, watch)) = inner.watches.iter_mut().find(|(_, watch)| watch.path == path) {
            if mask & IN_MASK_CREATE != 0 {
                return Err(ErrNo::EEXIST);
            }
            let event_mask = mask & (IN_ALL_EVENTS | IN_ONESHOT | IN_EXCL_UNLINK);
            if mask & IN_MASK_ADD != 0 {
                watch.mask |= event_mask;
            } else {
                watch.mask = event_mask;
            }
            return Ok(wd);
        }
        let wd = allocate_watch_descriptor(&mut inner)?;
        inner.watches.insert(wd,
                             Watch { path,
                                     mask : mask & (IN_ALL_EVENTS | IN_ONESHOT |
                                                    IN_EXCL_UNLINK) });
        Ok(wd)
    }

    fn remove_watch(&self, wd : i32) -> Result<(), ErrNo> {
        let mut inner = self.inner.lock();
        if inner.watches.remove(&wd).is_none() {
            return Err(ErrNo::EINVAL);
        }
        queue_event_locked(&mut inner, wd, IN_IGNORED, 0, "");
        drop(inner);
        self.wait.wake_all();
        Ok(())
    }

    fn notify_path(&self,
                   path : &str,
                   child_mask : u32,
                   self_mask : u32,
                   is_dir : bool,
                   cookie : u32,
                   remove_exact : bool) {
        let parent = parent_path(path);
        let name = base_name(path);
        let type_bit = if is_dir { IN_ISDIR } else { 0 };
        let mut inner = self.inner.lock();
        let mut deliveries = Vec::new();
        let mut remove = Vec::new();
        for (&wd, watch) in &inner.watches {
            let mut delivered = false;
            if watch.path == parent && watch.mask & child_mask != 0 {
                deliveries.push((wd, child_mask | type_bit, cookie, name.to_string()));
                delivered = true;
            }
            if watch.path == path && self_mask != 0 && watch.mask & self_mask != 0 {
                deliveries.push((wd, self_mask | type_bit, cookie, String::new()));
                delivered = true;
            }
            if remove_exact && watch.path == path {
                remove.push(wd);
            } else if delivered && watch.mask & IN_ONESHOT != 0 {
                remove.push(wd);
            }
        }
        for (wd, mask, event_cookie, event_name) in deliveries {
            queue_event_locked(&mut inner, wd, mask, event_cookie, event_name.as_str());
        }
        remove.sort_unstable();
        remove.dedup();
        for wd in remove {
            if inner.watches.remove(&wd).is_some() {
                queue_event_locked(&mut inner, wd, IN_IGNORED, 0, "");
            }
        }
        let wake = !inner.queue.is_empty();
        drop(inner);
        if wake {
            self.wait.wake_all();
        }
    }

    fn notify_move(&self, old_path : &str, new_path : &str, is_dir : bool, cookie : u32) {
        let old_parent = parent_path(old_path);
        let new_parent = parent_path(new_path);
        let old_name = base_name(old_path);
        let new_name = base_name(new_path);
        let type_bit = if is_dir { IN_ISDIR } else { 0 };
        let mut inner = self.inner.lock();
        let mut deliveries = Vec::new();
        let mut oneshot = Vec::new();
        for (&wd, watch) in &inner.watches {
            let mut delivered = false;
            if watch.path == old_parent && watch.mask & IN_MOVED_FROM != 0 {
                deliveries.push((wd, IN_MOVED_FROM | type_bit, old_name.to_string()));
                delivered = true;
            }
            if watch.path == new_parent && watch.mask & IN_MOVED_TO != 0 {
                deliveries.push((wd, IN_MOVED_TO | type_bit, new_name.to_string()));
                delivered = true;
            }
            if watch.path == old_path && watch.mask & IN_MOVE_SELF != 0 {
                deliveries.push((wd, IN_MOVE_SELF | type_bit, String::new()));
                delivered = true;
            }
            if delivered && watch.mask & IN_ONESHOT != 0 {
                oneshot.push(wd);
            }
        }
        for watch in inner.watches.values_mut() {
            if watch.path == old_path {
                watch.path = new_path.to_string();
            }
        }
        for (wd, mask, name) in deliveries {
            queue_event_locked(&mut inner, wd, mask, cookie, name.as_str());
        }
        oneshot.sort_unstable();
        oneshot.dedup();
        for wd in oneshot {
            if inner.watches.remove(&wd).is_some() {
                queue_event_locked(&mut inner, wd, IN_IGNORED, 0, "");
            }
        }
        let wake = !inner.queue.is_empty();
        drop(inner);
        if wake {
            self.wait.wake_all();
        }
    }
}

impl Drop for InotifyState {
    fn drop(&mut self) {
        self.wait.wake_all();
        let _ = self.wait.try_release_empty();
    }
}

fn allocate_watch_descriptor(inner : &mut InotifyInner) -> Result<i32, ErrNo> {
    for _ in 0..i32::MAX {
        let candidate = inner.next_wd.max(1);
        inner.next_wd = candidate.checked_add(1).unwrap_or(1);
        if !inner.watches.contains_key(&candidate) {
            return Ok(candidate);
        }
    }
    Err(ErrNo::ENOSPC)
}

fn queue_event_locked(inner : &mut InotifyInner,
                      wd : i32,
                      mask : u32,
                      cookie : u32,
                      name : &str) {
    if inner.queue.len() >= MAX_QUEUED_EVENTS {
        if !inner.overflow_queued {
            inner.queue.pop_front();
            if let Ok(bytes) = encode_event(-1, IN_Q_OVERFLOW, 0, "") {
                inner.queue.push_back(QueuedEvent { bytes });
                inner.overflow_queued = true;
            }
        }
        return;
    }
    if let Ok(bytes) = encode_event(wd, mask, cookie, name) {
        inner.queue.push_back(QueuedEvent { bytes });
    }
}

fn encode_event(wd : i32, mask : u32, cookie : u32, name : &str) -> Result<Vec<u8>, ()> {
    let name_len = if name.is_empty() { 0 } else { (name.len() + 1 + 3) & !3 };
    let total = EVENT_HEADER_LEN.checked_add(name_len).ok_or(())?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(total).map_err(|_| ())?;
    bytes.extend_from_slice(&wd.to_ne_bytes());
    bytes.extend_from_slice(&mask.to_ne_bytes());
    bytes.extend_from_slice(&cookie.to_ne_bytes());
    bytes.extend_from_slice(&(name_len as u32).to_ne_bytes());
    if name_len != 0 {
        bytes.extend_from_slice(name.as_bytes());
        bytes.resize(total, 0);
    }
    Ok(bytes)
}

struct InotifyHandle {
    state : Arc<InotifyState>,
}

impl VfsIoHandle for InotifyHandle {
    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        if max_len < EVENT_HEADER_LEN {
            return Err(VfsError::InvalidPath);
        }
        Ok(Box::new(InotifyPreparedRead { state : self.state.clone(), max_len }))
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(VfsMetadata { node_type : VfsNodeType::Special,
                         size : 0,
                         mode : 0o600,
                         device_major : 0,
                         device_minor : 0,
                         inode : Arc::as_ptr(&self.state) as usize as u64,
                         mount_id : 0,
                         nlink : 1,
                         uid : 0,
                         gid : 0 })
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self { state : self.state.clone() }))
    }

    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
        Ok(if events & POLLIN != 0 && self.state.has_events() { POLLIN } else { 0 })
    }

    fn poll_wait_for_ticks(&mut self,
                           events : i16,
                           timeout_ticks : u64,
                           still_waiting : &mut dyn FnMut() -> bool)
                           -> VfsResult<()> {
        if events & POLLIN == 0 || self.state.has_events() || timeout_ticks == 0 {
            return Ok(());
        }
        let result = self.state.wait.wait_current_while_for_ticks(timeout_ticks, || {
            still_waiting() && !self.state.has_events()
        });
        if result == task::TaskWaitResult::Interrupted && !self.state.has_events() {
            Err(VfsError::Interrupted)
        } else {
            Ok(())
        }
    }

    fn open_status_flags(&self) -> u32 {
        if self.state.inner.lock().nonblocking { IN_NONBLOCK as u32 } else { 0 }
    }

    fn open_accmode(&self) -> u32 { 0 }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        self.state.inner.lock().nonblocking = flags & IN_NONBLOCK as u32 != 0;
        Ok(())
    }
}

struct InotifyPreparedRead {
    state : Arc<InotifyState>,
    max_len : usize,
}

impl VfsPreparedRead for InotifyPreparedRead {
    fn acquire(self : Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        loop {
            let mut inner = self.state.inner.lock();
            if let Some(first) = inner.queue.front() {
                if first.bytes.len() > self.max_len {
                    return Err(VfsError::InvalidPath);
                }
                let mut events = Vec::new();
                let mut total = 0usize;
                while let Some(event) = inner.queue.front() {
                    let Some(next_total) = total.checked_add(event.bytes.len()) else { break; };
                    if next_total > self.max_len {
                        break;
                    }
                    let event = inner.queue.pop_front().expect("front event must exist");
                    if event.bytes.get(4..8) == Some(IN_Q_OVERFLOW.to_ne_bytes().as_slice()) {
                        inner.overflow_queued = false;
                    }
                    total = next_total;
                    events.push(event);
                }
                drop(inner);
                return InotifyReadLease::new(self.state.clone(), events)
                    .map(|lease| Box::new(lease) as Box<dyn VfsReadLease>);
            }
            if inner.nonblocking {
                return Err(VfsError::WouldBlock);
            }
            drop(inner);
            let result = self.state.wait.wait_current_while(|| !self.state.has_events());
            if result == task::TaskWaitResult::Interrupted && !self.state.has_events() {
                return Err(VfsError::Interrupted);
            }
        }
    }
}

struct InotifyReadLease {
    state : Arc<InotifyState>,
    events : Vec<QueuedEvent>,
    event_ends : Vec<usize>,
    bytes : Vec<u8>,
    finished : bool,
}

impl InotifyReadLease {
    fn new(state : Arc<InotifyState>, events : Vec<QueuedEvent>) -> VfsResult<Self> {
        let total = events.iter().try_fold(0usize, |sum, event| {
            sum.checked_add(event.bytes.len()).ok_or(VfsError::NoMemory)
        })?;
        let mut bytes = Vec::new();
        let mut event_ends = Vec::new();
        if bytes.try_reserve_exact(total).is_err() ||
           event_ends.try_reserve_exact(events.len()).is_err()
        {
            restore_events(&state, events);
            return Err(VfsError::NoMemory);
        }
        for event in &events {
            bytes.extend_from_slice(&event.bytes);
            event_ends.push(bytes.len());
        }
        Ok(Self { state, events, event_ends, bytes, finished : false })
    }

    fn restore_from(&mut self, first : usize) {
        if first >= self.events.len() {
            return;
        }
        let events = self.events.drain(first..).collect();
        restore_events(&self.state, events);
    }
}

impl VfsReadLease for InotifyReadLease {
    fn bytes(&self) -> &[u8] { &self.bytes }

    fn finish(mut self : Box<Self>, progress : VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied > self.bytes.len() {
            return Err(VfsError::Io);
        }
        let committed = if progress.complete {
            self.events.len()
        } else {
            self.event_ends.iter().take_while(|end| **end <= progress.copied).count()
        };
        let copied = self.event_ends.get(committed.wrapping_sub(1)).copied().unwrap_or(0);
        self.restore_from(committed);
        self.finished = true;
        if copied > 0 || progress.complete {
            Ok(VfsReadFinish::Bytes(copied))
        } else {
            Ok(VfsReadFinish::Fault)
        }
    }
}

impl Drop for InotifyReadLease {
    fn drop(&mut self) {
        if !self.finished {
            self.restore_from(0);
        }
    }
}

fn restore_events(state : &Arc<InotifyState>, mut events : Vec<QueuedEvent>) {
    if events.is_empty() {
        return;
    }
    let mut inner = state.inner.lock();
    while let Some(event) = events.pop() {
        inner.queue.push_front(event);
    }
    drop(inner);
    state.wait.wake_all();
}

pub(crate) fn sys_inotify_init1(args : SyscallArgs) -> UserRet {
    let flags = args.arg(0);
    if flags & !(IN_NONBLOCK | IN_CLOEXEC) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let fd_number = match fd::alloc_fd(Box::new(InotifyHandle {
        state : InotifyState::new(flags & IN_NONBLOCK != 0),
    })) {
        Ok(fd_number) => fd_number,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    if flags & IN_CLOEXEC != 0 {
        if let Err(error) = fd::set_fd_flags(fd_number, FD_CLOEXEC) {
            let _ = fd::close_fd(fd_number);
            return UserRet::from_error(vfs_error_to_errno(error));
        }
    }
    UserRet::from_success(fd_number)
}

pub(crate) fn sys_inotify_add_watch(args : SyscallArgs) -> UserRet {
    let fd_number = args.arg(0);
    let path = match copy_user_path_cstr(args.arg(1), crate::user_copy::USER_PATH_MAX) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    let mask = args.arg(2) as u32;
    if mask & IN_ALL_EVENTS == 0 || mask & !VALID_WATCH_MASK != 0 ||
       mask & IN_MASK_ADD != 0 && mask & IN_MASK_CREATE != 0
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let resolved = match resolve_path_at(AT_FDCWD, path.as_str()) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    let resolved = match resolve_symlinks(resolved.as_str(),
                                          if mask & IN_DONT_FOLLOW != 0 {
                                              FinalSymlink::NoFollow
                                          } else {
                                              FinalSymlink::Follow
                                          }) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    let metadata = match active_impl::backend().metadata(resolved.as_str()) {
        Ok(metadata) => metadata,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    if mask & IN_ONLYDIR != 0 && metadata.node_type != VfsNodeType::Directory {
        return UserRet::from_error(ErrNo::ENOTDIR);
    }
    match fd::with_current_io(fd_number, |handle| {
        let inotify = handle.as_any().downcast_ref::<InotifyHandle>()
                            .ok_or(VfsError::InvalidPath)?;
        inotify.state.add_watch(resolved, mask).map_err(errno_to_vfs)
    }) {
        Ok(wd) => UserRet::from_success(wd as usize),
        Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
    }
}

pub(crate) fn sys_inotify_rm_watch(args : SyscallArgs) -> UserRet {
    let fd_number = args.arg(0);
    let wd = args.arg(1) as i32;
    match fd::with_current_io(fd_number, |handle| {
        let inotify = handle.as_any().downcast_ref::<InotifyHandle>()
                            .ok_or(VfsError::InvalidPath)?;
        inotify.state.remove_watch(wd).map_err(errno_to_vfs)
    }) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
    }
}

fn errno_to_vfs(error : ErrNo) -> VfsError {
    match error {
        ErrNo::EEXIST => VfsError::Exists,
        ErrNo::ENOSPC => VfsError::NoSpace,
        _ => VfsError::InvalidPath,
    }
}

fn active_instances() -> Vec<Arc<InotifyState>> {
    let mut registry = INSTANCES.lock();
    let instances = registry.iter().filter_map(Weak::upgrade).collect();
    registry.retain(|entry| entry.strong_count() != 0);
    instances
}

fn notify(path : &str,
          child_mask : u32,
          self_mask : u32,
          is_dir : bool,
          remove_exact : bool) {
    for instance in active_instances() {
        instance.notify_path(path, child_mask, self_mask, is_dir, 0, remove_exact);
    }
}

pub(crate) fn notify_open(path : &str, is_dir : bool) {
    notify(path, IN_OPEN, IN_OPEN, is_dir, false);
}

pub(crate) fn notify_modify(path : &str) {
    notify(path, IN_MODIFY, IN_MODIFY, false, false);
}

pub(crate) fn notify_attrib(path : &str, is_dir : bool) {
    notify(path, IN_ATTRIB, IN_ATTRIB, is_dir, false);
}

pub(crate) fn notify_create(path : &str, is_dir : bool) {
    notify(path, IN_CREATE, 0, is_dir, false);
}

pub(crate) fn notify_delete(path : &str, is_dir : bool) {
    notify(path, IN_DELETE, IN_DELETE_SELF, is_dir, true);
}

pub(crate) fn notify_move(old_path : &str, new_path : &str, is_dir : bool) {
    let mut cookie = NEXT_RENAME_COOKIE.fetch_add(1, Ordering::Relaxed);
    if cookie == 0 {
        cookie = NEXT_RENAME_COOKIE.fetch_add(1, Ordering::Relaxed);
    }
    for instance in active_instances() {
        instance.notify_move(old_path, new_path, is_dir, cookie);
    }
}

pub(crate) fn notify_fd_modify(fd_number : usize) {
    let path = fd::with_current_io(fd_number, |handle| {
        Ok(handle.backing_path().map(String::from))
    }).ok().flatten();
    if let Some(path) = path {
        notify_modify(path.as_str());
    }
}

fn parent_path(path : &str) -> &str {
    path.rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/")
}

fn base_name(path : &str) -> &str {
    path.rsplit_once('/').map(|(_, name)| name).unwrap_or(path)
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    let event = encode_event(7, IN_CREATE, 11, "abc").expect("encode inotify event");
    assert_eq!(event.len(), 20);
    assert_eq!(i32::from_ne_bytes(event[0..4].try_into().unwrap()), 7);
    assert_eq!(u32::from_ne_bytes(event[4..8].try_into().unwrap()), IN_CREATE);
    assert_eq!(u32::from_ne_bytes(event[8..12].try_into().unwrap()), 11);
    assert_eq!(u32::from_ne_bytes(event[12..16].try_into().unwrap()), 4);
    assert_eq!(&event[16..20], b"abc\0");
}
