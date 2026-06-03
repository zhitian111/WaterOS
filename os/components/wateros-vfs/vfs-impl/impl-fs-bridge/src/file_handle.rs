//! ext4 根卷普通文件句柄：小文件整文件缓冲，大文件走页缓存（见 [`super::paged_handle`]）。

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use api_v0::{
    SingleRootReadView, VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsOpenFlags, VfsResult,
    VfsSeekWhence,
};
use fs::{FsAccessMode, FsKind};

use crate::map_fs_err;
use crate::FsBridge;

/// 已打开的根卷普通文件（小文件：全文缓冲于内存）。
#[derive(Clone)]
pub struct BufferedFileHandle {
    path: String,
    data: Vec<u8>,
    offset: u64,
    meta: VfsMetadata,
    writable: bool,
    dirty: bool,
}

/// 与历史命名兼容的别名。
pub type RootFileHandle = BufferedFileHandle;

impl BufferedFileHandle {
    pub(crate) fn open(
        bridge: &FsBridge,
        path: String,
        flags: VfsOpenFlags,
    ) -> VfsResult<Self> {
        let want_read = flags.contains(VfsOpenFlags::READ)
            || (!flags.contains(VfsOpenFlags::WRITE) && !flags.contains(VfsOpenFlags::CREATE));
        let want_write = flags.contains(VfsOpenFlags::WRITE);
        if !want_read && !want_write {
            return Err(VfsError::Unsupported);
        }

        let exists = bridge.exists(path.as_str())?;
        if !exists {
            if !flags.contains(VfsOpenFlags::CREATE) {
                return Err(VfsError::NotFound);
            }
            if !want_write {
                return Err(VfsError::Unsupported);
            }
            let meta = VfsMetadata {
                node_type: VfsNodeType::File,
                size: 0,
                mode: 0o100644,
            };
            return Ok(Self {
                path,
                data: Vec::new(),
                offset: 0,
                meta,
                writable: true,
                dirty: true,
            });
        }

        let meta = bridge.metadata(path.as_str())?;
        if meta.node_type != VfsNodeType::File {
            return Err(VfsError::NotAFile);
        }

        let mut data = if want_read || want_write {
            bridge.read(path.as_str())?
        } else {
            Vec::new()
        };

        let mut dirty = false;
        if flags.contains(VfsOpenFlags::TRUNC) && want_write {
            data.clear();
            dirty = true;
        }

        let mut offset = 0u64;
        if flags.contains(VfsOpenFlags::APPEND) {
            offset = data.len() as u64;
        }

        let mut meta = meta;
        meta.size = data.len() as u64;

        Ok(Self {
            path,
            data,
            offset,
            meta,
            writable: want_write,
            dirty,
        })
    }

    pub(crate) fn open_boxed(
        bridge: &FsBridge,
        path: String,
        flags: VfsOpenFlags,
    ) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self::open(bridge, path, flags)?))
    }

    fn sync_dirty(&mut self) -> VfsResult<()> {
        if !self.dirty {
            return Ok(());
        }
        let imp = fs::pick_fs_impl(FsKind::Ext4, FsAccessMode::ReadWrite)
            .ok_or(VfsError::Unsupported)?;
        let dev_path = fs::rootfs::active_impl::current_root_device_path()
            .ok_or(VfsError::NotMounted)?;
        let dev = fs::devfs::active_impl::lookup_block_device(dev_path.as_str())
            .map_err(map_fs_err)?;
        let rw = imp.mount_rw(dev).map_err(map_fs_err)?;
        rw.lock()
            .write_regular_file(self.path.as_str(), &self.data)
            .map_err(map_fs_err)?;
        self.dirty = false;
        self.meta.size = self.data.len() as u64;
        Ok(())
    }
}

impl VfsIoHandle for BufferedFileHandle {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let off = usize::try_from(self.offset).map_err(|_| VfsError::Io)?;
        if off >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - off);
        buf[..n].copy_from_slice(&self.data[off..off + n]);
        self.offset = self
            .offset
            .checked_add(n as u64)
            .ok_or(VfsError::Io)?;
        Ok(n)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let off = usize::try_from(self.offset).map_err(|_| VfsError::Io)?;
        let end = off.checked_add(buf.len()).ok_or(VfsError::Io)?;
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[off..end].copy_from_slice(buf);
        self.offset = end as u64;
        self.meta.size = self.data.len() as u64;
        self.dirty = true;
        Ok(buf.len())
    }

    fn close(&mut self) -> VfsResult<()> {
        self.sync_dirty()
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        let mut m = self.meta.clone();
        m.size = self.data.len() as u64;
        Ok(m)
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let off = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        if off >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - off);
        buf[..n].copy_from_slice(&self.data[off..off + n]);
        Ok(n)
    }

    fn write_at(&mut self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let off = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        let end = off.checked_add(buf.len()).ok_or(VfsError::Io)?;
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[off..end].copy_from_slice(buf);
        self.meta.size = self.data.len() as u64;
        self.dirty = true;
        Ok(buf.len())
    }

    fn seek(&mut self, offset: i64, whence: VfsSeekWhence) -> VfsResult<u64> {
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

    fn flush(&mut self) -> VfsResult<()> {
        self.sync_dirty()
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(self.clone()))
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
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
    pub(crate) fn open_path(
        &self,
        path: &str,
        flags: VfsOpenFlags,
    ) -> VfsResult<Box<dyn VfsIoHandle>> {
        let abs = api_v0::resolve_open_path(path)?;
        if flags.contains(VfsOpenFlags::DIRECTORY) {
            return super::dir_handle::DirectoryHandle::open(self, abs);
        }
        super::paged_handle::open_file(self, abs, flags)
    }
}
