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
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use api_v0::{
    normalize_absolute_path, SingleRootReadView, VfsError, VfsIoHandle, VfsMetadata,
    VfsOpenDescriptionState, VfsOpenFlags, VfsPreparedRead, VfsReadLease, VfsResult,
    VfsSeekWhence,
};
use impl_page_cache::{global_cache, PageCacheIo};
use spin::Mutex;
use wateros_base_config::fs::{FileIoMode, FILE_IO_MODE};

use crate::read_lease::{try_zeroed, ReservationGuard, StagedReadLease};
use crate::{map_fs_err, mount_table::{resolve_route, FsRoute}, root_rw, FsBridge};

/// detached 模式下单文件内核堆缓冲上限。
// 本变量代码由AI完成
const DETACHED_DATA_MAX : usize = 16 * 1024 * 1024;
static NEXT_FLOCK_OWNER_ID : AtomicU64 = AtomicU64::new(1);

struct DetachedState {
    detached : bool,
    data : Vec<u8>,
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
pub(crate) struct FsPageIo;

impl PageCacheIo for FsPageIo {
    type Error = VfsError;

// 本方法代码由AI完成
    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> Result<usize, VfsError> {
        FsBridge.read_range(path, offset, buf)
    }

// 本方法代码由AI完成
    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> Result<usize, VfsError> {
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
            global_cache(self.mount_gen).acquire_open_ref(self.path.as_str());
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

        let mut on_disk_size = meta.size;
        if flags.contains(VfsOpenFlags::TRUNC) && want_write {
            crate::replace_file_contents(path.as_str(), &[])?;
            meta = bridge.metadata(path.as_str())?;
            cache.truncate(path.as_str(), 0);
            on_disk_size = 0;
        }

        let mut offset = 0u64;
        if flags.contains(VfsOpenFlags::APPEND) {
            offset = cache.logical_size(path.as_str(), on_disk_size);
        }

        let mut meta = meta;
        meta.size = cache.logical_size(path.as_str(), on_disk_size);

        cache.acquire_open_ref(path.as_str());

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
                  detached : Arc::new(Mutex::new(DetachedState { detached : false,
                                                                 data : Vec::new() })),
                  open_ref_held : true,
                  flock_owner_id : NEXT_FLOCK_OWNER_ID.fetch_add(1, Ordering::Relaxed) })
    }

// 本方法代码由AI完成
    fn release_open_ref_if_held(&mut self) {
        if self.open_ref_held {
            global_cache(self.mount_gen).release_open_ref(self.path.as_str());
            self.open_ref_held = false;
        }
    }

// 本方法代码由AI完成
    fn sync_dirty(&mut self) -> VfsResult<()> {
        if !self.writable || self.is_detached() {
            log::trace!("[vfs-flush] skip path={} writable={} detached={}",
                        self.path,
                        self.writable,
                        self.is_detached());
            return Ok(());
        }
        let mut io = FsPageIo;
        let cache = global_cache(self.mount_gen);
        let had_dirty_pages = cache.dirty_page_count(self.path.as_str()) != 0;
        match cache.flush(&mut io,
                          self.path.as_str(),
                          core::convert::identity)
        {
            Ok(()) if had_dirty_pages => crate::sync_path_filesystem(self.path.as_str()),
            Ok(()) => Ok(()),
            Err(VfsError::NotFound) => {
                self.mark_detached();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

// 本方法代码由AI完成
    fn current_size(&self) -> u64 {
        global_cache(self.mount_gen).logical_size(self.path.as_str(), self.on_disk_size)
    }

    fn is_detached(&self) -> bool { self.detached.lock().detached }

    fn mark_detached(&self) { self.detached.lock().detached = true; }

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
        let cache = global_cache(self.mount_gen);
        cache.set_logical_size(self.path.as_str(), new_size);
        self.meta.size = new_size;
        Ok(buf.len())
    }
}

impl VfsIoHandle for PagedFileHandle {
    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        let reservation = ReservationGuard::begin(self.description.clone())?;
        global_cache(self.mount_gen).acquire_open_ref(self.path.as_str());
        Ok(Box::new(PagedPreparedRead {
            reservation : Some(reservation),
            path : self.path.clone(),
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
                    self.path,
                    offset,
                    buf.len(),
                    size);
        let mut io = FsPageIo;
        let cache = global_cache(self.mount_gen);
        let n = cache.read(&mut io,
                           self.path.as_str(),
                           size,
                           offset,
                           buf,
                           core::convert::identity)?;
        log::trace!("[paged_handle] read OK path={} offset={} n={}/{}",
                    self.path,
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
        let mut io = FsPageIo;
        let cache = global_cache(self.mount_gen);
        let n = match cache.write(&mut io,
                                  self.path.as_str(),
                                  size,
                                  offset,
                                  buf,
                                  core::convert::identity)
        {
            Ok(n) => n,
            Err(VfsError::NotFound) => {
                self.mark_detached();
                let n = self.account_detached_write(offset, buf)?;
                reservation.commit(n, n)?;
                return Ok(n);
            }
            Err(e) => return Err(e),
        };
        reservation.commit(n, n)?;
        self.meta.size = self.current_size();
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
        let mut io = FsPageIo;
        let cache = global_cache(self.mount_gen);
        let n = cache.read(&mut io,
                           self.path.as_str(),
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
        let mut io = FsPageIo;
        let cache = global_cache(self.mount_gen);
        let n = match cache.write(&mut io,
                                  self.path.as_str(),
                                  size,
                                  offset,
                                  buf,
                                  core::convert::identity)
        {
            Ok(n) => n,
            Err(VfsError::NotFound) => {
                self.mark_detached();
                return self.account_detached_write(offset, buf);
            }
            Err(e) => return Err(e),
        };
        self.meta.size = self.current_size();
        Ok(n)
    }

// 本方法代码由AI完成
    fn close(&mut self) -> VfsResult<()> {
        let sync_err = self.sync_dirty();
        self.release_open_ref_if_held();
        if let Err(e) = &sync_err {
            log::warn!("[paged_handle] close sync_dirty failed path={:?} err={e:?}",
                       self.path);
        }
        sync_err
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        let mut m = match FsBridge.metadata(self.path.as_str()) {
            Ok(meta) => meta,
            Err(_) => self.meta.clone(),
        };
        m.size = self.current_size();
        Ok(m)
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
            match self.sync_dirty() {
                Ok(()) => {}
                Err(VfsError::NotFound) => self.mark_detached(),
                Err(e) => return Err(e),
            }
        }
        if !self.is_detached() {
            let n = normalize_absolute_path(self.path.as_str())?;
            match resolve_route(self.path.as_str())? {
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
                FsRoute::AuxRo { .. } | FsRoute::PseudoProc { .. } | FsRoute::PseudoSecurity { .. } => {
                    return Err(VfsError::ReadOnlyFs);
                }
            }
        }
        let cache = global_cache(self.mount_gen);
        cache.truncate(self.path.as_str(), len);
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
    path : String,
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
        let cache = global_cache(self.mount_gen);
        let size = cache.logical_size(self.path.as_str(), self.on_disk_size);
        let available = size.saturating_sub(offset);
        let len = usize::try_from(available.min(self.max_len as u64)).map_err(|_| VfsError::Io)?;
        let mut staged = try_zeroed(len)?;
        let n = if len == 0 {
            0
        } else {
            let detached = self.detached.lock();
            if detached.detached {
                let start = usize::try_from(offset).map_err(|_| VfsError::Io)?;
                let n = len.min(detached.data.len().saturating_sub(start));
                staged[..n].copy_from_slice(&detached.data[start..start + n]);
                n
            } else {
                drop(detached);
                let mut io = FsPageIo;
                cache.read(&mut io,
                           self.path.as_str(),
                           size,
                           offset,
                           staged.as_mut_slice(),
                           core::convert::identity)?
            }
        };
        staged.truncate(n);
        let reservation = self.reservation.take().ok_or(VfsError::Io)?;
        Ok(Box::new(StagedReadLease::new(reservation, staged)))
    }
}

impl Drop for PagedPreparedRead {
    fn drop(&mut self) {
        if self.open_ref_held {
            global_cache(self.mount_gen).release_open_ref(self.path.as_str());
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
        if !want_write {
            return Err(VfsError::Unsupported);
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
