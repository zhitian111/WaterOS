//! 大文件句柄：经全局页缓存 Direct 读写，不在 `open` 时整文件载入 RAM。

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use api_v0::{
    RootRwSession, SingleRootReadView, VfsError, VfsIoHandle, VfsMetadata, VfsOpenFlags, VfsResult,
    VfsSeekWhence,
};
use impl_page_cache::{global_cache, PageCacheIo};
use wateros_base_config::fs::{FileIoMode, FILE_IO_MODE};

use crate::{FsBridge, MountedRwSession};

/// 委托根卷 RO/RW 区间 I/O，供页缓存 flush 与 miss 加载使用。
pub(crate) struct FsPageIo<'a> {
    pub rw : Option<&'a mut MountedRwSession>,
}

impl PageCacheIo for FsPageIo<'_> {
    type Error = VfsError;

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> Result<usize, VfsError> {
        FsBridge.read_range(path, offset, buf)
    }

    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> Result<usize, VfsError> {
        let Some(rw) = self.rw
                           .as_deref_mut()
        else {
            return Err(VfsError::Unsupported);
        };
        rw.write_range(path, offset, data)
    }
}

fn mount_rw_session() -> VfsResult<MountedRwSession> {
    let rw = fs::rootfs::active_impl::root_rw_fs().ok_or(VfsError::NotMounted)?;
    Ok(MountedRwSession::new(rw))
}

/// 页缓存-backed 根卷普通文件句柄。
#[derive(Clone)]
pub struct PagedFileHandle {
    path : String,
    offset : u64,
    meta : VfsMetadata,
    writable : bool,
    mount_gen : u64,
    on_disk_size : u64,
    detached : bool,
    detached_data : Vec<u8>,
}

impl PagedFileHandle {
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

        Ok(Self { path,
                  offset,
                  meta,
                  writable : want_write,
                  mount_gen,
                  on_disk_size,
                  detached : false,
                  detached_data : Vec::new() })
    }

    fn sync_dirty(&mut self) -> VfsResult<()> {
        if !self.writable || self.detached {
            return Ok(());
        }
        let mut rw = mount_rw_session()?;
        let mut io = FsPageIo { rw : Some(&mut rw) };
        let cache = global_cache(self.mount_gen);
        match cache.flush(&mut io,
                          self.path.as_str(),
                          core::convert::identity)
        {
            Ok(()) => Ok(()),
            Err(VfsError::NotFound) => {
                self.detached = true;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn current_size(&self) -> u64 {
        global_cache(self.mount_gen).logical_size(self.path.as_str(), self.on_disk_size)
    }

    fn read_detached_at(&self, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        let start = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        if start >=
           self.detached_data
               .len()
        {
            return Ok(0);
        }
        let n = core::cmp::min(buf.len(),
                               self.detached_data
                                   .len() -
                               start);
        buf[..n].copy_from_slice(&self.detached_data[start..start + n]);
        Ok(n)
    }

    fn account_detached_write(&mut self,
                              offset : u64,
                              buf : &[u8],
                              advance_offset : bool)
                              -> VfsResult<usize> {
        let start = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        let end = offset.checked_add(buf.len() as u64)
                        .ok_or(VfsError::Io)?;
        let end_usize = usize::try_from(end).map_err(|_| VfsError::Io)?;
        if self.detached_data
               .len() <
           end_usize
        {
            self.detached_data
                .resize(end_usize, 0);
        }
        self.detached_data[start..end_usize].copy_from_slice(buf);
        if advance_offset {
            self.offset = end;
        }
        let new_size = core::cmp::max(self.current_size(), end);
        let cache = global_cache(self.mount_gen);
        cache.set_logical_size(self.path.as_str(), new_size);
        self.meta.size = new_size;
        Ok(buf.len())
    }
}

impl VfsIoHandle for PagedFileHandle {
    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let size = self.current_size();
        if self.offset >= size {
            return Ok(0);
        }
        if self.detached {
            let n = self.read_detached_at(self.offset, buf)?;
            self.offset = self.offset
                              .checked_add(n as u64)
                              .ok_or(VfsError::Io)?;
            return Ok(n);
        }
        // iozone 调试：页缓存读前
        log::trace!("[paged_handle] read path={} offset={} len={} size={}",
                    self.path,
                    self.offset,
                    buf.len(),
                    size);
        let mut io = FsPageIo { rw : None };
        let cache = global_cache(self.mount_gen);
        let n = cache.read(&mut io,
                           self.path.as_str(),
                           size,
                           self.offset,
                           buf,
                           core::convert::identity)?;
        // iozone 调试：页缓存读后
        log::trace!("[paged_handle] read OK path={} offset={} n={}/{}",
                    self.path,
                    self.offset,
                    n,
                    buf.len());
        self.offset = self.offset
                          .checked_add(n as u64)
                          .ok_or(VfsError::Io)?;
        Ok(n)
    }

    fn write(&mut self, buf : &[u8]) -> VfsResult<usize> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        if self.detached {
            return self.account_detached_write(self.offset, buf, true);
        }
        let size = self.current_size();
        let mut rw = mount_rw_session()?;
        let mut io = FsPageIo { rw : Some(&mut rw) };
        let cache = global_cache(self.mount_gen);
        let n = match cache.write(&mut io,
                                  self.path.as_str(),
                                  size,
                                  self.offset,
                                  buf,
                                  core::convert::identity)
        {
            Ok(n) => n,
            Err(VfsError::NotFound) => {
                self.detached = true;
                return self.account_detached_write(self.offset, buf, true);
            }
            Err(e) => return Err(e),
        };
        self.offset = self.offset
                          .checked_add(n as u64)
                          .ok_or(VfsError::Io)?;
        self.meta.size = self.current_size();
        Ok(n)
    }

    fn read_at(&mut self, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let size = self.current_size();
        if offset >= size {
            return Ok(0);
        }
        if self.detached {
            return self.read_detached_at(offset, buf);
        }
        let mut io = FsPageIo { rw : None };
        let cache = global_cache(self.mount_gen);
        let n = cache.read(&mut io,
                           self.path.as_str(),
                           size,
                           offset,
                           buf,
                           core::convert::identity)?;
        Ok(n)
    }

    fn write_at(&mut self, offset : u64, buf : &[u8]) -> VfsResult<usize> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        if self.detached {
            return self.account_detached_write(offset, buf, false);
        }
        let size = self.current_size();
        let mut rw = mount_rw_session()?;
        let mut io = FsPageIo { rw : Some(&mut rw) };
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
                self.detached = true;
                return self.account_detached_write(offset, buf, false);
            }
            Err(e) => return Err(e),
        };
        self.meta.size = self.current_size();
        Ok(n)
    }

    fn close(&mut self) -> VfsResult<()> {
        // 将脏页刷回磁盘（不 purge files 条目，后续 open 可复用）。
        self.sync_dirty()
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        let mut m = self.meta.clone();
        m.size = self.current_size();
        Ok(m)
    }

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
                if offset < 0 {
                    self.offset
                        .checked_sub((-offset) as u64)
                        .ok_or(VfsError::InvalidPath)?
                } else {
                    self.offset
                        .checked_add(offset as u64)
                        .ok_or(VfsError::InvalidPath)?
                }
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
        self.offset = new_off;
        Ok(new_off)
    }

    fn flush(&mut self) -> VfsResult<()> { self.sync_dirty() }

    fn truncate(&mut self, len : u64) -> VfsResult<()> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        if len > 0 {
            match self.sync_dirty() {
                Ok(()) => {}
                Err(VfsError::NotFound) => self.detached = true,
                Err(e) => return Err(e),
            }
        }
        if !self.detached {
            let mut rw = mount_rw_session()?;
            match rw.truncate(self.path.as_str(), len) {
                Ok(()) => {}
                Err(VfsError::NotFound) => self.detached = true,
                Err(e) => return Err(e),
            }
        }
        let cache = global_cache(self.mount_gen);
        cache.truncate(self.path.as_str(), len);
        self.on_disk_size = len;
        self.meta.size = len;
        if self.offset > len {
            self.offset = len;
        }
        if self.detached {
            let new_len = usize::try_from(len).map_err(|_| VfsError::Io)?;
            self.detached_data
                .resize(new_len, 0);
        }
        Ok(())
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> { Ok(Box::new(self.clone())) }

    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
        const POLLIN : i16 = 0x001;
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

/// 打开根卷普通文件。所有普通文件都走页缓存/range I/O，避免 benchmark
/// 逐步扩展文件时反复整文件读写。
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
        return Err(VfsError::NotAFile);
    }

    let h = PagedFileHandle::open(bridge, path, flags, meta)?;
    Ok(Box::new(h))
}
