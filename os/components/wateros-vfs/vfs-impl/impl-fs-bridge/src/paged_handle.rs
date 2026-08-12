//! 大文件句柄：经全局页缓存 Direct 读写，不在 `open` 时整文件载入 RAM。
//!
//! ## Lock ordering（与 [`impl_page_cache`] 一致）
//!
//! 1. `page_cache.files`（极短）
//! 2. per-file `FileEntryInner` RwLock（`entry.read` / `entry.write`）
//! 3. `page_cache.state`（极短；持锁期间不得调 ext4）
//! 4. `SharedRwFs`（仅在 `FsPageIo::read_range` / `write_range` 内短持有）
//!
//! 禁止在持有 ext4 锁后再等待页缓存 entry 锁（写/fsync 路径曾因此与读 miss 死锁）。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use api_v0::{
    normalize_absolute_path, SingleRootReadView, VfsError, VfsFileContentIdentity, VfsIoHandle,
    VfsMetadata, VfsOpenDescriptionState, VfsOpenFlags, VfsPreparedRead, VfsReadLease, VfsResult,
    VfsSeekWhence,
};
use impl_page_cache::{global_cache, FileCacheKey, PageCacheIo, PinnedFileRead};
use spin::Mutex;
use wateros_base_config::fs::{FileIoMode, FILE_IO_MODE};
use fs::{FsError, FsNodeId, SharedRwFs};

use crate::read_lease::{try_zeroed, ReservationGuard, StagedReadLease};
use crate::{map_fs_err, mount_table::{resolve_route, FsRoute, MountIdentity}, root_rw, FsBridge};

/// detached 模式下单文件内核堆缓冲上限。
// 本变量代码由AI完成
const DETACHED_DATA_MAX : usize = 16 * 1024 * 1024;
static NEXT_FLOCK_OWNER_ID : AtomicU64 = AtomicU64::new(1);

struct StableNodeLease {
    fs : SharedRwFs,
    node : FsNodeId,
    identity : MountIdentity,
    content_identity : VfsFileContentIdentity,
}

impl StableNodeLease {
    fn cache_key(&self) -> String {
        alloc::format!("@node:{:016x}:{:016x}", self.identity.mount_id, self.node.raw())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        self.fs.lock()
               .metadata_node(self.node)
               .map(|meta| crate::map_meta(meta, self.identity))
               .map_err(map_fs_err)
    }

    fn read_range(&self, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        self.fs.lock()
               .read_range_node(self.node, offset, buf)
               .map_err(map_fs_err)
    }

    fn write_range(&self, offset : u64, data : &[u8]) -> VfsResult<usize> {
        let mut done = 0usize;
        while done < data.len() {
            let written = self.fs.lock()
                                  .write_range_node(self.node,
                                                    offset + done as u64,
                                                    &data[done..])
                                  .map_err(map_fs_err)?;
            if written == 0 {
                return Err(VfsError::Io);
            }
            done = done.checked_add(written).ok_or(VfsError::Io)?;
        }
        Ok(done)
    }

    fn truncate(&self, len : u64) -> VfsResult<()> {
        self.fs.lock().truncate_node(self.node, len).map_err(map_fs_err)
    }

    fn sync(&self) -> VfsResult<()> { self.fs.lock().sync().map_err(map_fs_err) }

    fn mark_content_changed(&self) { self.content_identity.mark_changed(); }
}

impl Drop for StableNodeLease {
    fn drop(&mut self) {
        if let Err(error) = self.fs.lock().close_node(self.node) {
            log::warn!("[paged_handle] stable node close failed node={} mount={} err={error:?}",
                       self.node.raw(),
                       self.identity.mount_id);
        }
    }
}

type StableContentKey = (u64, u64, u64);
static CONTENT_VERSIONS : Mutex<BTreeMap<StableContentKey, Weak<AtomicU64>>> =
    Mutex::new(BTreeMap::new());

fn stable_content_identity(mount_gen : u64,
                           identity : MountIdentity,
                           node : FsNodeId)
                           -> VfsFileContentIdentity {
    let key = (mount_gen, identity.mount_id, node.raw());
    let mut versions = CONTENT_VERSIONS.lock();
    let version = versions.get(&key)
                          .and_then(Weak::upgrade)
                          .unwrap_or_else(|| {
                              let version = Arc::new(AtomicU64::new(1));
                              versions.insert(key, Arc::downgrade(&version));
                              version
                          });
    VfsFileContentIdentity::new(mount_gen,
                                identity.mount_id,
                                node.raw(),
                                version)
}

fn open_stable_node(mount_gen : u64, path : &str) -> VfsResult<Option<Arc<StableNodeLease>>> {
    let (fs, rel, identity) = match resolve_route(path)? {
        FsRoute::Root { abs, identity } => (root_rw()?, abs, identity),
        FsRoute::AuxRw { fs, rel, identity, .. } => (fs, rel, identity),
        FsRoute::AuxRo { .. } | FsRoute::PseudoProc { .. } | FsRoute::PseudoSecurity { .. } => {
            return Ok(None);
        }
    };
    let node = match fs.lock().open_node(rel.as_str()) {
        Ok(node) => node,
        Err(FsError::Unsupported) => return Ok(None),
        Err(error) => return Err(map_fs_err(error)),
    };
    let content_identity = stable_content_identity(mount_gen, identity, node);
    Ok(Some(Arc::new(StableNodeLease { fs,
                                      node,
                                      identity,
                                      content_identity })))
}

struct DetachedState {
    detached : bool,
    path : String,
    data : Vec<u8>,
    stable : Option<Arc<StableNodeLease>>,
    cache_key : String,
}

fn file_key_for_state(mount_gen : u64, state : &DetachedState) -> FileCacheKey {
    let path = Arc::from(state.cache_key.as_str());
    match state.stable.as_ref() {
        Some(node) => FileCacheKey::stable(mount_gen,
                                           path,
                                           node.identity.mount_id,
                                           node.node.raw()),
        None => FileCacheKey::path(mount_gen, path),
    }
}

type DetachedKey = (u64, String);
static DETACHED_STATES : Mutex<BTreeMap<DetachedKey, Weak<Mutex<DetachedState>>>> =
    Mutex::new(BTreeMap::new());
type StableNodeKey = (u64, String);
static STABLE_NODES : Mutex<BTreeMap<StableNodeKey, Weak<StableNodeLease>>> =
    Mutex::new(BTreeMap::new());
static STABLE_NODE_REGISTRATIONS : AtomicU64 = AtomicU64::new(0);

fn register_stable_node(mount_gen : u64, stable : &Arc<StableNodeLease>) {
    let mut nodes = STABLE_NODES.lock();
    if STABLE_NODE_REGISTRATIONS.fetch_add(1, Ordering::Relaxed) & 0xff == 0 {
        nodes.retain(|_, node| node.strong_count() != 0);
    }
    nodes.insert((mount_gen, stable.cache_key()), Arc::downgrade(stable));
}

fn stable_node_for_cache_key(mount_gen : u64,
                             cache_key : &str)
                             -> Option<Arc<StableNodeLease>> {
    let key = (mount_gen, String::from(cache_key));
    let mut nodes = STABLE_NODES.lock();
    let stable = nodes.get(&key).and_then(Weak::upgrade);
    if stable.is_none() {
        nodes.remove(&key);
    }
    stable
}

fn detached_state_for_open(mount_gen : u64,
                           path : &str,
                           stable : Option<Arc<StableNodeLease>>)
                           -> Arc<Mutex<DetachedState>> {
    let key = (mount_gen, String::from(path));
    let mut states = DETACHED_STATES.lock();
    if let Some(state) = states.get(&key).and_then(Weak::upgrade) {
        if let Some(stable) = state.lock().stable.as_ref() {
            register_stable_node(mount_gen, stable);
        }
        return state;
    }
    let cache_key = stable.as_ref()
                          .map(|node| node.cache_key())
                          .unwrap_or_else(|| String::from(path));
    let state = Arc::new(Mutex::new(DetachedState { detached : false,
                                                    path : String::from(path),
                                                    data : Vec::new(),
                                                    stable,
                                                    cache_key }));
    if let Some(stable) = state.lock().stable.as_ref() {
        register_stable_node(mount_gen, stable);
    }
    states.insert(key, Arc::downgrade(&state));
    state
}

pub(crate) struct PendingUnlinkDetach {
    key : DetachedKey,
    state : Arc<Mutex<DetachedState>>,
    data : Option<Vec<u8>>,
}

impl PendingUnlinkDetach {
    pub(crate) fn commit(self) {
        DETACHED_STATES.lock().remove(&self.key);
        let mut state = self.state.lock();
        if let Some(data) = self.data {
            state.data = data;
            state.detached = true;
        }
    }
}

pub(crate) fn commit_rename_state(old_path : &str,
                                  new_path : &str,
                                  replaced : Option<PendingUnlinkDetach>) {
    let mount_gen = fs::rootfs::active_impl::mount_generation();
    let old_key = (mount_gen, String::from(old_path));
    let new_key = (mount_gen, String::from(new_path));
    let mut states = DETACHED_STATES.lock();
    let source = states.remove(&old_key).and_then(|state| state.upgrade());
    states.remove(&new_key);

    let (target_state, mut target_data) = match replaced {
        Some(replaced) => (Some(replaced.state), Some(replaced.data)),
        None => (None, None),
    };
    let mut target = target_state.as_ref().map(|state| state.lock());
    let mut source_state = source.as_ref().map(|source| source.lock());
    global_cache(mount_gen).finish_rename(old_path, new_path);
    if let (Some(target), Some(Some(data))) = (target.as_mut(), target_data.take()) {
        target.data = data;
        target.detached = true;
    }
    if let (Some(source), Some(source_state)) = (source.as_ref(), source_state.as_mut()) {
        source_state.path = String::from(new_path);
        if source_state.stable.is_none() {
            source_state.cache_key = String::from(new_path);
        }
        states.insert(new_key, Arc::downgrade(&source));
    }
}

pub(crate) fn prepare_unlink_detach(path : &str) -> VfsResult<Option<PendingUnlinkDetach>> {
    let mount_gen = fs::rootfs::active_impl::mount_generation();
    let key = (mount_gen, String::from(path));
    let state = {
        let mut states = DETACHED_STATES.lock();
        let state = states.get(&key).and_then(Weak::upgrade);
        if state.is_none() {
            states.remove(&key);
        }
        state
    };
    let Some(state) = state else {
        if let Some(stable) = open_stable_node(mount_gen, path)? {
            stable.mark_content_changed();
        }
        return Ok(None);
    };
    if state.lock().detached {
        return Ok(None);
    }

    let (meta, cache_key, stable) = {
        let state = state.lock();
        let meta = match state.stable.as_ref() {
            Some(node) => node.metadata()?,
            None => FsBridge.metadata(path)?,
        };
        (meta, state.cache_key.clone(), state.stable.clone())
    };
    if let Some(stable) = stable.as_ref() {
        stable.mark_content_changed();
        return Ok(Some(PendingUnlinkDetach { key, state, data : None }));
    }
    let cache = global_cache(mount_gen);
    let logical_size = cache.logical_size(cache_key.as_str(), meta.size);
    let len = usize::try_from(logical_size).map_err(|_| VfsError::Io)?;
    check_detached_len(len)?;
    let mut data = try_zeroed(len)?;
    if len != 0 {
        let mut io = FsPageIo::new(mount_gen, stable);
        let read = cache.read(&mut io,
                              cache_key.as_str(),
                              logical_size,
                              0,
                              &mut data,
                              core::convert::identity)?;
        if read != len {
            return Err(VfsError::Io);
        }
    }
    Ok(Some(PendingUnlinkDetach { key, state, data : Some(data) }))
}

// 本方法代码由AI完成
fn check_detached_len(len : usize) -> VfsResult<()> {
    if len > DETACHED_DATA_MAX {
        log::warn!("[paged_handle] detached buffer cap exceeded len={} max={}",
                   len,
                   DETACHED_DATA_MAX);
        return Err(VfsError::Io);
    }
    Ok(())
}

// 本方法代码由AI完成
fn grow_detached_data(buf : &mut Vec<u8>, new_len : usize) -> VfsResult<()> {
    check_detached_len(new_len)?;
    if buf.len() < new_len {
        buf.try_reserve_exact(new_len - buf.len())
           .map_err(|_| VfsError::Io)?;
        buf.resize(new_len, 0);
    }
    Ok(())
}

/// 页缓存 miss / flush 时下探根卷的 I/O 委托；ext4 锁在每次 `read_range`/`write_range` 内按需短持。
pub(crate) struct FsPageIo {
    mount_gen : u64,
    stable : Option<Arc<StableNodeLease>>,
    stable_key : Option<String>,
}

impl FsPageIo {
    pub(crate) fn path() -> Self {
        Self { mount_gen : fs::rootfs::active_impl::mount_generation(),
               stable : None,
               stable_key : None }
    }

    fn new(mount_gen : u64, stable : Option<Arc<StableNodeLease>>) -> Self {
        let stable_key = stable.as_ref().map(|node| node.cache_key());
        Self { mount_gen,
               stable,
               stable_key }
    }

    fn stable_for_key(&self, cache_key : &str) -> Option<Arc<StableNodeLease>> {
        if self.stable_key.as_deref() == Some(cache_key) {
            return self.stable.clone();
        }
        stable_node_for_cache_key(self.mount_gen, cache_key)
    }
}

impl PageCacheIo for FsPageIo {
    type Error = VfsError;

// 本方法代码由AI完成
    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> Result<usize, VfsError> {
        if let Some(node) = self.stable_for_key(path) {
            return node.read_range(offset, buf);
        }
        if path.starts_with("@node:") {
            return Err(VfsError::NotFound);
        }
        FsBridge.read_range(path, offset, buf)
    }

// 本方法代码由AI完成
    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> Result<usize, VfsError> {
        if let Some(node) = self.stable_for_key(path) {
            return node.write_range(offset, data);
        }
        if path.starts_with("@node:") {
            return Err(VfsError::NotFound);
        }
        match resolve_route(path)? {
            FsRoute::Root { abs, .. } => {
                let n = normalize_absolute_path(abs.as_str())?;
                let rw = root_rw()?;
                let mut done = 0usize;
                while done < data.len() {
                    let written = rw.lock()
                                    .write_range(n.as_str(),
                                                 offset + done as u64,
                                                 &data[done..])
                                    .map_err(map_fs_err)?;
                    if written == 0 {
                        return Err(VfsError::Io);
                    }
                    done = done.checked_add(written)
                               .ok_or(VfsError::Io)?;
                }
                Ok(done)
            }
            FsRoute::AuxRw { fs, rel, .. } => fs.lock()
                                                .write_range(rel.as_str(), offset, data)
                                                .map_err(map_fs_err),
            FsRoute::AuxRo { .. } | FsRoute::PseudoProc { .. } | FsRoute::PseudoSecurity { .. } => {
                Err(VfsError::ReadOnlyFs)
            }
        }
    }
}

/// 页缓存-backed 根卷普通文件句柄。
// 本结构代码由AI完成
pub struct PagedFileHandle {
    path : String,
    description : Arc<VfsOpenDescriptionState>,
    meta : VfsMetadata,
    writable : bool,
    accmode : u32,
    mount_gen : u64,
    on_disk_size : u64,
    detached : Arc<Mutex<DetachedState>>,
    open_ref_held : bool,
    flock_owner_id : u64,
}

impl Clone for PagedFileHandle {
// 本方法代码由AI完成
    fn clone(&self) -> Self {
        if self.open_ref_held {
            let key = self.cache_file_key();
            global_cache(self.mount_gen).acquire_open_ref_key(&key);
        }
        Self { path : self.path.clone(),
               description : self.description.clone(),
               meta : self.meta.clone(),
               writable : self.writable,
               accmode : self.accmode,
               mount_gen : self.mount_gen,
               on_disk_size : self.on_disk_size,
               detached : self.detached.clone(),
               open_ref_held : self.open_ref_held,
               flock_owner_id : self.flock_owner_id }
    }
}

impl Drop for PagedFileHandle {
    fn drop(&mut self) {
        if !self.open_ref_held {
            return;
        }
        let writeback_err = self.writeback_dirty();
        self.release_open_ref_if_held();
        if let Err(error) = writeback_err {
            log::warn!("[paged_handle] drop writeback failed path={:?} err={error:?}",
                       self.path);
        }
    }
}

impl PagedFileHandle {
// 本方法代码由AI完成
    pub(crate) fn open(bridge : &FsBridge,
                       path : String,
                       flags : VfsOpenFlags,
                       mut meta : VfsMetadata)
                       -> VfsResult<Self> {
        match FILE_IO_MODE {
            FileIoMode::Direct => {}
            FileIoMode::Async => return Err(VfsError::Unsupported),
        }

        let want_write = flags.contains(VfsOpenFlags::WRITE);
        let mount_gen = fs::rootfs::active_impl::mount_generation();
        let cache = global_cache(mount_gen);
        let stable = open_stable_node(mount_gen, path.as_str())?;
        let detached = detached_state_for_open(mount_gen, path.as_str(), stable);
        let (stable, cache_file_key) = {
            let state = detached.lock();
            (state.stable.clone(), file_key_for_state(mount_gen, &state))
        };

        let mut on_disk_size = meta.size;
        if flags.contains(VfsOpenFlags::TRUNC) && want_write {
            match stable.as_ref() {
                Some(node) => node.truncate(0)?,
                None => crate::truncate_path(path.as_str(), 0)?,
            }
            if let Some(node) = stable.as_ref() {
                node.mark_content_changed();
            }
            meta = match stable.as_ref() {
                Some(node) => node.metadata()?,
                None => bridge.metadata(path.as_str())?,
            };
            cache.truncate_key(&cache_file_key, 0);
            on_disk_size = 0;
        }

        let mut offset = 0u64;
        if flags.contains(VfsOpenFlags::APPEND) {
            offset = cache.logical_size_key(&cache_file_key, on_disk_size);
        }

        let mut meta = meta;
        meta.size = cache.logical_size_key(&cache_file_key, on_disk_size);

        cache.acquire_open_ref_key(&cache_file_key);

// 本变量代码由AI完成
        const O_WRONLY : u32 = 1;
// 本变量代码由AI完成
        const O_RDWR : u32 = 2;
// 本变量代码由AI完成
        const O_APPEND : u32 = 0o2000;
        let accmode = if want_write && flags.contains(VfsOpenFlags::READ) {
            O_RDWR
        } else if want_write {
            O_WRONLY
        } else {
            0
        };
        let mut status_flags = 0u32;
        if flags.contains(VfsOpenFlags::APPEND) {
            status_flags |= O_APPEND;
        }

        Ok(Self { path,
                  description : Arc::new(VfsOpenDescriptionState::new(offset, status_flags)),
                  meta,
                  writable : want_write,
                  accmode,
                  mount_gen,
                  on_disk_size,
                  detached,
                  open_ref_held : true,
                  flock_owner_id : NEXT_FLOCK_OWNER_ID.fetch_add(1, Ordering::Relaxed) })
    }

// 本方法代码由AI完成
    fn release_open_ref_if_held(&mut self) {
        if self.open_ref_held {
            let key = self.cache_file_key();
            global_cache(self.mount_gen).release_open_ref_key(&key);
            self.open_ref_held = false;
        }
    }

// 本方法代码由AI完成
    /// 将当前文件的脏页写入文件系统缓存，以便 close 后回收 VFS 页缓存条目；
    /// 不把普通 close(2) 扩大为整个文件系统的 fsync。
    fn writeback_dirty(&mut self) -> VfsResult<()> {
        if !self.writable || self.is_detached() {
            log::trace!("[vfs-flush] skip path={} writable={} detached={}",
                        self.path,
                        self.writable,
                        self.is_detached());
            return Ok(());
        }
        let key = self.cache_file_key();
        let mut io = self.page_io();
        let cache = global_cache(self.mount_gen);
        match cache.flush_key(&mut io,
                              &key,
                              core::convert::identity)
        {
            Ok(()) => Ok(()),
            Err(VfsError::NotFound) if self.stable_node().is_none() => {
                self.mark_detached();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Persist this file's data and the filesystem metadata beneath it.
    fn sync_dirty(&mut self) -> VfsResult<()> {
        self.writeback_dirty()?;
        if !self.writable || self.is_detached() {
            return Ok(());
        }
        let path = self.active_path();
        match self.stable_node() {
            Some(node) => node.sync(),
            None => crate::sync_path_filesystem(path.as_str()),
        }
    }

// 本方法代码由AI完成
    fn current_size(&self) -> u64 {
        let detached = self.detached.lock();
        if detached.detached {
            return detached.data.len() as u64;
        }
        drop(detached);
        // 句柄打开/截断时已记录磁盘大小；页缓存自身的逻辑大小会在本内核
        // 的写入和截断路径同步更新，因此读路径不必再逐次锁 ext4 metadata。
        let key = file_key_for_state(self.mount_gen, &self.detached.lock());
        global_cache(self.mount_gen).logical_size_key(&key, self.on_disk_size)
    }

    fn active_path(&self) -> String { self.detached.lock().path.clone() }

    fn cache_file_key(&self) -> FileCacheKey {
        file_key_for_state(self.mount_gen, &self.detached.lock())
    }

    fn stable_node(&self) -> Option<Arc<StableNodeLease>> {
        self.detached.lock().stable.clone()
    }

    fn page_io(&self) -> FsPageIo { FsPageIo::new(self.mount_gen, self.stable_node()) }

    fn is_detached(&self) -> bool { self.detached.lock().detached }

    fn mark_detached(&self) { self.detached.lock().detached = true; }

    fn mark_content_changed(&self) {
        if let Some(node) = self.stable_node() {
            node.mark_content_changed();
        }
    }

// 本方法代码由AI完成
    fn read_detached_at(&self, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        let start = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        let detached = self.detached.lock();
        if start >= detached.data.len() {
            return Ok(0);
        }
        let n = core::cmp::min(buf.len(), detached.data.len() - start);
        buf[..n].copy_from_slice(&detached.data[start..start + n]);
        Ok(n)
    }

// 本方法代码由AI完成
    fn account_detached_write(&mut self,
                              offset : u64,
                              buf : &[u8])
                              -> VfsResult<usize> {
        let start = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        let end = offset.checked_add(buf.len() as u64)
                        .ok_or(VfsError::Io)?;
        let end_usize = usize::try_from(end).map_err(|_| VfsError::Io)?;
        {
            let mut detached = self.detached.lock();
            if detached.data.len() < end_usize {
                grow_detached_data(&mut detached.data, end_usize)?;
            }
            detached.data[start..end_usize].copy_from_slice(buf);
        }
        let new_size = core::cmp::max(self.current_size(), end);
        self.meta.size = new_size;
        Ok(buf.len())
    }
}

impl VfsIoHandle for PagedFileHandle {
    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        let reservation = ReservationGuard::begin(self.description.clone())?;
        let key = self.cache_file_key();
        global_cache(self.mount_gen).acquire_open_ref_key(&key);
        Ok(Box::new(PagedPreparedRead {
            reservation : Some(reservation),
            mount_gen : self.mount_gen,
            on_disk_size : self.on_disk_size,
            detached : self.detached.clone(),
            max_len,
            open_ref_held : true,
        }))
    }

// 本方法代码由AI完成
    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut reservation = ReservationGuard::begin(self.description.clone())?;
        let size = self.current_size();
        let offset = reservation.offset();
        if offset >= size {
            reservation.commit(0, 0)?;
            return Ok(0);
        }
        if self.is_detached() {
            let n = self.read_detached_at(offset, buf)?;
            reservation.commit(n, n)?;
            return Ok(n);
        }
        log::trace!("[paged_handle] read path={} offset={} len={} size={}",
                    self.active_path(),
                    offset,
                    buf.len(),
                    size);
        let mut io = self.page_io();
        let cache = global_cache(self.mount_gen);
        let key = self.cache_file_key();
        let n = cache.read_key(&mut io,
                               &key,
                               size,
                               offset,
                               buf,
                               core::convert::identity)?;
        log::trace!("[paged_handle] read OK path={} offset={} n={}/{}",
                    self.active_path(),
                    offset,
                    n,
                    buf.len());
        reservation.commit(n, n)?;
        Ok(n)
    }

// 本方法代码由AI完成
    fn write(&mut self, buf : &[u8]) -> VfsResult<usize> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
// 本变量代码由AI完成
        if buf.is_empty() {
            return Ok(0);
        }
        const O_APPEND : u32 = 0o2000;
        let mut reservation = ReservationGuard::begin(self.description.clone())?;
        if self.description.status_flags() & O_APPEND != 0 {
            reservation.retarget(self.current_size())?;
        }
        let offset = reservation.offset();
        if self.is_detached() {
            let n = self.account_detached_write(offset, buf)?;
            reservation.commit(n, n)?;
            return Ok(n);
        }
        let size = self.current_size();
        let mut io = self.page_io();
        let cache = global_cache(self.mount_gen);
        let key = self.cache_file_key();
        let n = match cache.write_key(&mut io,
                                      &key,
                                      size,
                                      offset,
                                      buf,
                                      core::convert::identity)
        {
            Ok(n) => n,
            Err(VfsError::NotFound) if self.stable_node().is_none() => {
                self.mark_detached();
                let n = self.account_detached_write(offset, buf)?;
                reservation.commit(n, n)?;
                return Ok(n);
            }
            Err(e) => return Err(e),
        };
        reservation.commit(n, n)?;
        self.meta.size = self.current_size();
        if n != 0 {
            self.mark_content_changed();
        }
        Ok(n)
    }

// 本方法代码由AI完成
    fn read_at(&mut self, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let size = self.current_size();
        if offset >= size {
            return Ok(0);
        }
        if self.is_detached() {
            return self.read_detached_at(offset, buf);
        }
        let mut io = self.page_io();
        let cache = global_cache(self.mount_gen);
        let key = self.cache_file_key();
        let n = cache.read_key(&mut io,
                               &key,
                               size,
                               offset,
                               buf,
                               core::convert::identity)?;
        Ok(n)
    }

// 本方法代码由AI完成
    fn write_at(&mut self, offset : u64, buf : &[u8]) -> VfsResult<usize> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        if self.is_detached() {
            return self.account_detached_write(offset, buf);
        }
        let size = self.current_size();
        let mut io = self.page_io();
        let cache = global_cache(self.mount_gen);
        let key = self.cache_file_key();
        let n = match cache.write_key(&mut io,
                                      &key,
                                      size,
                                      offset,
                                      buf,
                                      core::convert::identity)
        {
            Ok(n) => n,
            Err(VfsError::NotFound) if self.stable_node().is_none() => {
                self.mark_detached();
                return self.account_detached_write(offset, buf);
            }
            Err(e) => return Err(e),
        };
        self.meta.size = self.current_size();
        if n != 0 {
            self.mark_content_changed();
        }
        Ok(n)
    }

// 本方法代码由AI完成
    fn close(&mut self) -> VfsResult<()> {
        let writeback_err = self.writeback_dirty();
        self.release_open_ref_if_held();
        if let Err(e) = &writeback_err {
            log::warn!("[paged_handle] close writeback failed path={:?} err={e:?}",
                       self.path);
        }
        writeback_err
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        let path = self.active_path();
        let mut m = match self.stable_node() {
            Some(node) if !self.is_detached() => node.metadata().unwrap_or_else(|_| self.meta.clone()),
            _ => match FsBridge.metadata(path.as_str()) {
                Ok(meta) => meta,
                Err(_) => self.meta.clone(),
            },
        };
        m.size = self.current_size();
        Ok(m)
    }

    fn file_content_identity(&self) -> Option<VfsFileContentIdentity> {
        if self.is_detached() {
            return None;
        }
        self.stable_node().map(|node| node.content_identity.clone())
    }

// 本方法代码由AI完成
    fn backing_path(&self) -> Option<&str> {
        Some(self.path.as_str())
    }

// 本方法代码由AI完成
    fn flock_owner_id(&self) -> Option<u64> {
        Some(self.flock_owner_id)
    }

// 本方法代码由AI完成
    fn seek(&mut self, offset : i64, whence : VfsSeekWhence) -> VfsResult<u64> {
        let size = self.current_size();
        let new_off = match whence {
            VfsSeekWhence::Set => {
                if offset < 0 {
                    return Err(VfsError::InvalidPath);
                }
                offset as u64
            }
            VfsSeekWhence::Cur => {
                return self.description.add_signed_offset_if_idle(offset);
            }
            VfsSeekWhence::End => {
                if offset < 0 {
                    size.checked_sub((-offset) as u64)
                        .ok_or(VfsError::InvalidPath)?
                } else {
                    size.checked_add(offset as u64)
                        .ok_or(VfsError::InvalidPath)?
                }
            }
        };
        self.description.set_offset_if_idle(new_off)
    }

// 本方法代码由AI完成
    fn flush(&mut self) -> VfsResult<()> {
        self.sync_dirty()
    }

// 本方法代码由AI完成
    fn truncate(&mut self, len : u64) -> VfsResult<()> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        let mut reservation = ReservationGuard::begin(self.description.clone())?;
        if len > 0 {
            match self.writeback_dirty() {
                Ok(()) => {}
                Err(VfsError::NotFound) => self.mark_detached(),
                Err(e) => return Err(e),
            }
        }
        if !self.is_detached() {
            if let Some(node) = self.stable_node() {
                node.truncate(len)?;
            } else {
                let path = self.active_path();
                let n = normalize_absolute_path(path.as_str())?;
                match resolve_route(path.as_str())? {
                    FsRoute::Root { .. } => {
                        match root_rw()?.lock().truncate(n.as_str(), len).map_err(map_fs_err) {
                            Ok(()) => {}
                            Err(VfsError::NotFound) => self.mark_detached(),
                            Err(e) => return Err(e),
                        }
                    }
                    FsRoute::AuxRw { fs, rel, .. } => {
                        match fs.lock().truncate(rel.as_str(), len).map_err(map_fs_err) {
                            Ok(()) => {}
                            Err(VfsError::NotFound) => self.mark_detached(),
                            Err(e) => return Err(e),
                        }
                    }
                    FsRoute::AuxRo { .. } | FsRoute::PseudoProc { .. } |
                    FsRoute::PseudoSecurity { .. } => return Err(VfsError::ReadOnlyFs),
                }
            }
        }
        let cache = global_cache(self.mount_gen);
        if !self.is_detached() {
            let key = self.cache_file_key();
            cache.truncate_key(&key, len);
        }
        self.on_disk_size = len;
        self.meta.size = len;
        if self.is_detached() {
            let new_len = usize::try_from(len).map_err(|_| VfsError::Io)?;
            let mut detached = self.detached.lock();
            if new_len > detached.data.len() {
                grow_detached_data(&mut detached.data, new_len)?;
            } else {
                detached.data.truncate(new_len);
            }
        }
        reservation.commit_at(reservation.offset().min(len))?;
        self.mark_content_changed();
        Ok(())
    }

// 本方法代码由AI完成
    fn open_status_flags(&self) -> u32 {
        self.description.status_flags()
    }

// 本方法代码由AI完成
    fn open_accmode(&self) -> u32 {
        self.accmode
    }

// 本方法代码由AI完成
    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
// 本变量代码由AI完成
        const O_APPEND : u32 = 0o2000;
// 本变量代码由AI完成
        const O_NONBLOCK : u32 = 0o4000;
        self.description.set_status_flags(flags & (O_APPEND | O_NONBLOCK));
        Ok(())
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> { Ok(Box::new(self.clone())) }

// 本方法代码由AI完成
    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
// 本变量代码由AI完成
        const POLLIN : i16 = 0x001;
// 本变量代码由AI完成
        const POLLOUT : i16 = 0x004;
        let mut revents = 0i16;
        if events & POLLIN != 0 {
            revents |= POLLIN;
        }
        if events & POLLOUT != 0 && self.writable {
            revents |= POLLOUT;
        }
        Ok(revents)
    }
}

struct PagedPreparedRead {
    reservation : Option<ReservationGuard>,
    mount_gen : u64,
    on_disk_size : u64,
    detached : Arc<Mutex<DetachedState>>,
    max_len : usize,
    open_ref_held : bool,
}

impl VfsPreparedRead for PagedPreparedRead {
    fn acquire(mut self : Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        let offset = self.reservation
                         .as_ref()
                         .ok_or(VfsError::Io)?
                         .offset();
        let detached = self.detached.lock();
        if detached.detached {
            let start = usize::try_from(offset).map_err(|_| VfsError::Io)?;
            let len = self.max_len.min(detached.data.len().saturating_sub(start));
            let mut staged = try_zeroed(len)?;
            staged.copy_from_slice(&detached.data[start..start + len]);
            let reservation = self.reservation.take().ok_or(VfsError::Io)?;
            return Ok(Box::new(StagedReadLease::new(reservation, staged)));
        } else {
            let key = file_key_for_state(self.mount_gen, &detached);
            let stable = detached.stable.clone();
            drop(detached);
            let cache = global_cache(self.mount_gen);
            let backing_size = stable.as_ref()
                                     .and_then(|node| node.metadata().ok())
                                     .map(|meta| meta.size)
                                     .unwrap_or(self.on_disk_size);
            let size = cache.logical_size_key(&key, backing_size);
            let available = size.saturating_sub(offset);
            let len = usize::try_from(available.min(self.max_len as u64)).map_err(|_| VfsError::Io)?;
            let mut io = FsPageIo::new(self.mount_gen, stable);
            let pinned = cache.pin_read_key(&mut io,
                                            &key,
                                            size,
                                            offset,
                                            len,
                                            || VfsError::NoMemory,
                                            core::convert::identity)?;
            let reservation = self.reservation.take().ok_or(VfsError::Io)?;
            Ok(Box::new(PinnedPagedReadLease { reservation,
                                               pinned }))
        }
    }
}

struct PinnedPagedReadLease {
    reservation : ReservationGuard,
    pinned : PinnedFileRead,
}

impl VfsReadLease for PinnedPagedReadLease {
    fn len(&self) -> usize { self.pinned.len() }

    fn visit(&self, visitor : &mut dyn FnMut(&[u8]) -> bool) {
        self.pinned.visit(visitor);
    }

    fn finish(mut self : Box<Self>, progress : api_v0::VfsCopyProgress)
              -> VfsResult<api_v0::VfsReadFinish> {
        if progress.copied > self.pinned.len() {
            return Err(VfsError::Io);
        }
        self.reservation.commit(progress.copied, self.pinned.len())?;
        if progress.copied == 0 && !progress.complete {
            Ok(api_v0::VfsReadFinish::Fault)
        } else {
            Ok(api_v0::VfsReadFinish::Bytes(progress.copied))
        }
    }
}

impl Drop for PagedPreparedRead {
    fn drop(&mut self) {
        if self.open_ref_held {
            let state = self.detached.lock();
            let key = file_key_for_state(self.mount_gen, &state);
            drop(state);
            global_cache(self.mount_gen).release_open_ref_key(&key);
            self.open_ref_held = false;
        }
    }
}

/// 打开根卷普通文件。所有普通文件都走页缓存/range I/O，避免 benchmark
/// 逐步扩展文件时反复整文件读写。
// 本方法代码由AI完成
pub(crate) fn open_file(bridge : &FsBridge,
                        path : String,
                        flags : VfsOpenFlags)
                        -> VfsResult<Box<dyn VfsIoHandle>> {
    let want_write = flags.contains(VfsOpenFlags::WRITE);

    if want_write || flags.contains(VfsOpenFlags::CREATE) {
        super::mount_table::assert_path_writable(path.as_str())?;
    }

    let exists = bridge.exists(path.as_str())?;
    if !exists {
        if !flags.contains(VfsOpenFlags::CREATE) {
            return Err(VfsError::NotFound);
        }
        crate::replace_file_contents(path.as_str(), &[])?;
        let meta = bridge.metadata(path.as_str())?;
        let h = PagedFileHandle::open(bridge, path, flags, meta)?;
        return Ok(Box::new(h));
    }

    let meta = bridge.metadata(path.as_str())?;
    if meta.node_type != api_v0::VfsNodeType::File {
        if meta.node_type == api_v0::VfsNodeType::Directory &&
           !want_write &&
           !flags.contains(VfsOpenFlags::TRUNC) &&
           !flags.contains(VfsOpenFlags::CREATE)
        {
            return super::dir_handle::DirectoryHandle::open(bridge, path);
        }
        return Err(VfsError::NotAFile);
    }

    let h = PagedFileHandle::open(bridge, path, flags, meta)?;
    Ok(Box::new(h))
}
