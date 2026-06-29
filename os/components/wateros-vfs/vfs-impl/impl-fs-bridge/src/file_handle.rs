//! ext4 根卷小文件缓冲句柄；普通 `open` 主路径已改由 [`super::paged_handle`] 处理。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::mount_table::{resolve_route, FsRoute};
use crate::{replace_file_contents, FsBridge};
use api_v0::{
    SingleRootReadView, VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsOpenFlags, VfsResult,
    VfsSeekWhence,
};

const S_IFMT: u16 = 0o170000;
const S_IFCHR: u16 = 0o020000;

static NEXT_FLOCK_OWNER_ID : AtomicU64 = AtomicU64::new(1);

/// 已打开的根卷普通文件（小文件：全文缓冲于内存）。
#[derive(Clone)]
// 本结构代码由AI完成
pub struct BufferedFileHandle {
    path : String,
    data : Vec<u8>,
    offset : u64,
    meta : VfsMetadata,
    writable : bool,
    dirty : bool,
    flock_owner_id : u64,
}

/// 与历史命名兼容的别名。
pub type RootFileHandle = BufferedFileHandle;

impl BufferedFileHandle {
// 本方法代码由AI完成
    pub(crate) fn open(bridge : &FsBridge, path : String, flags : VfsOpenFlags) -> VfsResult<Self> {
        let want_read = flags.contains(VfsOpenFlags::READ) ||
                        (!flags.contains(VfsOpenFlags::WRITE) &&
                         !flags.contains(VfsOpenFlags::CREATE));
        let want_write = flags.contains(VfsOpenFlags::WRITE);
        if !want_read && !want_write {
            return Err(VfsError::Unsupported);
        }

        let exists = bridge.exists(path.as_str())?;
        if !exists {
            if !flags.contains(VfsOpenFlags::CREATE) && !want_write {
                return Err(VfsError::NotFound);
            }
            if !want_write {
                return Err(VfsError::Unsupported);
            }
            replace_file_contents(path.as_str(), &[])?;
        }

        let mut meta = bridge.metadata(path.as_str())?;
        if meta.node_type != VfsNodeType::File {
            return Err(VfsError::NotAFile);
        }

        let mut data = if want_read || want_write {
            bridge.read(path.as_str())?
        } else {
            Vec::new()
        };

        let dirty = false;
        if flags.contains(VfsOpenFlags::TRUNC) && want_write {
            replace_file_contents(path.as_str(), &[])?;
            data.clear();
            meta = bridge.metadata(path.as_str())?;
        }

        let mut offset = 0u64;
        if flags.contains(VfsOpenFlags::APPEND) {
            offset = data.len() as u64;
        }

        let mut meta = meta;
        meta.size = data.len() as u64;

        Ok(Self { path,
                  data,
                  offset,
                  meta,
                  writable : want_write,
                  dirty,
                  flock_owner_id : NEXT_FLOCK_OWNER_ID.fetch_add(1, Ordering::Relaxed) })
    }

// 本方法代码由AI完成
    pub(crate) fn open_boxed(bridge : &FsBridge,
                             path : String,
                             flags : VfsOpenFlags)
                             -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self::open(bridge, path, flags)?))
    }

// 本方法代码由AI完成
    fn sync_dirty(&mut self) -> VfsResult<()> {
        if !self.dirty {
            return Ok(());
        }
        if !FsBridge.exists(self.path.as_str())? {
            self.dirty = false;
            return Ok(());
        }
        replace_file_contents(self.path.as_str(), &self.data)?;
        self.dirty = false;
        self.meta.size = self.data.len() as u64;
        Ok(())
    }
}

impl VfsIoHandle for BufferedFileHandle {
// 本方法代码由AI完成
    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let off = usize::try_from(self.offset).map_err(|_| VfsError::Io)?;
        if off >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len()
                   .min(self.data.len() - off);
        buf[..n].copy_from_slice(&self.data[off..off + n]);
        self.offset = self.offset
                          .checked_add(n as u64)
                          .ok_or(VfsError::Io)?;
        Ok(n)
    }

// 本方法代码由AI完成
    fn write(&mut self, buf : &[u8]) -> VfsResult<usize> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let off = usize::try_from(self.offset).map_err(|_| VfsError::Io)?;
        let end = off.checked_add(buf.len())
                     .ok_or(VfsError::Io)?;
        if end > self.data.len() {
            self.data
                .resize(end, 0);
        }
        self.data[off..end].copy_from_slice(buf);
        self.offset = end as u64;
        self.meta.size = self.data.len() as u64;
        self.dirty = true;
        Ok(buf.len())
    }

// 本方法代码由AI完成
    fn close(&mut self) -> VfsResult<()> { self.sync_dirty() }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        let mut m = match FsBridge.metadata(self.path.as_str()) {
            Ok(meta) => meta,
            Err(_) => self.meta.clone(),
        };
        m.size = self.data.len() as u64;
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
    fn open_accmode(&self) -> u32 {
        if self.writable {
            2
        } else {
            0
        }
    }

// 本方法代码由AI完成
    fn read_at(&mut self, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let off = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        if off >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len()
                   .min(self.data.len() - off);
        buf[..n].copy_from_slice(&self.data[off..off + n]);
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
        let off = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        let end = off.checked_add(buf.len())
                     .ok_or(VfsError::Io)?;
        if end > self.data.len() {
            self.data
                .resize(end, 0);
        }
        self.data[off..end].copy_from_slice(buf);
        self.meta.size = self.data.len() as u64;
        self.dirty = true;
        Ok(buf.len())
    }

// 本方法代码由AI完成
    fn truncate(&mut self, len : u64) -> VfsResult<()> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        let len = usize::try_from(len).map_err(|_| VfsError::InvalidPath)?;
        self.data
            .resize(len, 0);
        self.meta.size = len as u64;
        if self.offset > self.meta.size {
            self.offset = self.meta.size;
        }
        self.dirty = true;
        Ok(())
    }

// 本方法代码由AI完成
    fn seek(&mut self, offset : i64, whence : VfsSeekWhence) -> VfsResult<u64> {
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
                let base = self.data.len() as u64;
                if offset < 0 {
                    base.checked_sub((-offset) as u64)
                        .ok_or(VfsError::InvalidPath)?
                } else {
                    base.checked_add(offset as u64)
                        .ok_or(VfsError::InvalidPath)?
                }
            }
        };
        self.offset = new_off;
        Ok(new_off)
    }

// 本方法代码由AI完成
    fn flush(&mut self) -> VfsResult<()> { self.sync_dirty() }

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

impl FsBridge {
// 本方法代码由AI完成
    pub(crate) fn open_path(&self,
                            path : &str,
                            flags : VfsOpenFlags)
                            -> VfsResult<Box<dyn VfsIoHandle>> {
        let abs = if path.starts_with('/') {
            String::from(api_v0::normalize_absolute_path(path)?.as_str())
        } else {
            String::from(api_v0::resolve_open_path(path)?.as_str())
        };
        match resolve_route(abs.as_str())? {
            FsRoute::PseudoProc { rel, identity } => {
                return super::proc_handle::open_proc(rel, abs.clone(), flags, identity);
            }
            FsRoute::PseudoSecurity { rel, .. } if rel == "/" && flags.contains(VfsOpenFlags::DIRECTORY) => {
                return super::dir_handle::DirectoryHandle::open(self, abs);
            }
            FsRoute::PseudoSecurity { .. } => return Err(VfsError::NotFound),
            _ => {}
        }
        match abs.as_str() {
            "/dev/null" => return Ok(Box::new(impl_fd_session::NullDeviceHandle)),
            "/dev/zero" => return Ok(Box::new(impl_fd_session::ZeroDeviceHandle)),
            "/dev/random" | "/dev/urandom" => {
                return Ok(Box::new(impl_fd_session::UrandomDeviceHandle));
            }
            "/dev/cpu_dma_latency" => {
                return Ok(Box::new(impl_fd_session::CpuDmaLatencyDeviceHandle));
            }
            _ => {}
        }
        if let Ok(dev) = fs::devfs::active_impl::lookup_character_device(abs.as_str()) {
            return Ok(Box::new(impl_fd_session::CharDevHandle::from_devfs_path(dev,
                                                                               abs.as_str())));
        }
        if let Ok(meta) = self.metadata(abs.as_str()) {
            if meta.node_type == VfsNodeType::Special && meta.mode & S_IFMT == S_IFCHR {
                return Ok(Box::new(impl_fd_session::NullDeviceHandle));
            }
            if meta.node_type == VfsNodeType::Directory && !flags.contains(VfsOpenFlags::WRITE) {
                return super::dir_handle::DirectoryHandle::open(self, abs);
            }
        }
        if flags.contains(VfsOpenFlags::DIRECTORY) {
            return super::dir_handle::DirectoryHandle::open(self, abs);
        }
        match resolve_route(abs.as_str())? {
            FsRoute::AuxRw { readonly : true, .. } if flags.contains(VfsOpenFlags::WRITE) => {
                Err(VfsError::ReadOnlyFs)
            }
            FsRoute::AuxRo { .. } => Ok(Box::new(BufferedFileHandle::open(self, abs, flags)?)),
            _ => super::paged_handle::open_file(self, abs, flags),
        }
    }
}
