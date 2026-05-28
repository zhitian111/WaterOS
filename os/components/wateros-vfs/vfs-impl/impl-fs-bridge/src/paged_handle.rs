//! 大文件句柄：经全局页缓存 Direct 读写，不在 `open` 时整文件载入 RAM。

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;

use api_v0::{
    RootRwSession, SingleRootReadView, VfsError, VfsIoHandle, VfsMetadata, VfsOpenFlags,
    VfsResult, VfsSeekWhence,
};
use impl_page_cache::{global_cache, PageCacheIo};
use wateros_base_config::fs::{FileIoMode, FILE_IO_MODE, FILE_LARGE_THRESHOLD};

use crate::map_fs_err;
use crate::file_handle::BufferedFileHandle;
use crate::{FsBridge, MountedRwSession};

/// 委托根卷 RO/RW 区间 I/O，供页缓存 flush 与 miss 加载使用。
pub(crate) struct FsPageIo<'a> {
    pub rw: Option<&'a mut MountedRwSession>,
}

impl PageCacheIo for FsPageIo<'_> {
    type Error = VfsError;

    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        FsBridge.read_range(path, offset, buf)
    }

    fn write_range(
        &mut self,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<usize, VfsError> {
        let Some(rw) = self.rw.as_deref_mut() else {
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
    path: String,
    offset: u64,
    meta: VfsMetadata,
    writable: bool,
    mount_gen: u64,
    on_disk_size: u64,
}

impl PagedFileHandle {
    pub(crate) fn open(
        _bridge: &FsBridge,
        path: String,
        flags: VfsOpenFlags,
        meta: VfsMetadata,
    ) -> VfsResult<Self> {
        match FILE_IO_MODE {
            FileIoMode::Direct => {}
            FileIoMode::Async => return Err(VfsError::Unsupported),
        }

        let want_write = flags.contains(VfsOpenFlags::WRITE);
        let mount_gen = fs::rootfs::active_impl::mount_generation();
        let cache = global_cache(mount_gen);
        cache.set_logical_size(path.as_str(), meta.size);

        let mut on_disk_size = meta.size;
        if flags.contains(VfsOpenFlags::TRUNC) && want_write {
            cache.set_logical_size(path.as_str(), 0);
            on_disk_size = 0;
        }

        let mut offset = 0u64;
        if flags.contains(VfsOpenFlags::APPEND) {
            offset = cache.logical_size(path.as_str(), on_disk_size);
        }

        let mut meta = meta;
        meta.size = cache.logical_size(path.as_str(), on_disk_size);

        Ok(Self {
            path,
            offset,
            meta,
            writable: want_write,
            mount_gen,
            on_disk_size,
        })
    }

    fn sync_dirty(&mut self) -> VfsResult<()> {
        if !self.writable {
            return Ok(());
        }
        let mut rw = mount_rw_session()?;
        let mut io = FsPageIo { rw: Some(&mut rw) };
        let cache = global_cache(self.mount_gen);
        cache.flush(&mut io, self.path.as_str(), core::convert::identity)
    }

    fn current_size(&self) -> u64 {
        global_cache(self.mount_gen).logical_size(self.path.as_str(), self.on_disk_size)
    }
}

impl VfsIoHandle for PagedFileHandle {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let size = self.current_size();
        if self.offset >= size {
            return Ok(0);
        }
        let mut io = FsPageIo { rw: None };
        let cache = global_cache(self.mount_gen);
        let n = cache.read(
            &mut io,
            self.path.as_str(),
            size,
            self.offset,
            buf,
            core::convert::identity,
        )?;
        self.offset = self.offset.checked_add(n as u64).ok_or(VfsError::Io)?;
        Ok(n)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let size = self.current_size();
        let mut rw = mount_rw_session()?;
        let mut io = FsPageIo { rw: Some(&mut rw) };
        let cache = global_cache(self.mount_gen);
        let n = cache.write(
            &mut io,
            self.path.as_str(),
            size,
            self.offset,
            buf,
            core::convert::identity,
        )?;
        self.offset = self
            .offset
            .checked_add(n as u64)
            .ok_or(VfsError::Io)?;
        self.meta.size = self.current_size();
        Ok(n)
    }

    fn close(&mut self) -> VfsResult<()> {
        self.sync_dirty()
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        let mut m = self.meta.clone();
        m.size = self.current_size();
        Ok(m)
    }

    fn seek(&mut self, offset: i64, whence: VfsSeekWhence) -> VfsResult<u64> {
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

    fn flush(&mut self) -> VfsResult<()> {
        self.sync_dirty()
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(self.clone()))
    }
}

/// 按阈值在缓冲句柄与页缓存句柄间分流。
pub(crate) fn open_file(
    bridge: &FsBridge,
    path: String,
    flags: VfsOpenFlags,
) -> VfsResult<Box<dyn VfsIoHandle>> {
    let want_read = flags.contains(VfsOpenFlags::READ)
        || (!flags.contains(VfsOpenFlags::WRITE) && !flags.contains(VfsOpenFlags::CREATE));
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
        return BufferedFileHandle::open_boxed(bridge, path, flags);
    }

    let meta = bridge.metadata(path.as_str())?;
    if meta.node_type != api_v0::VfsNodeType::File {
        return Err(VfsError::NotAFile);
    }

    let use_paged = (want_read || want_write) && meta.size >= FILE_LARGE_THRESHOLD;

    if use_paged {
        let h = PagedFileHandle::open(bridge, path, flags, meta)?;
        return Ok(Box::new(h));
    }

    BufferedFileHandle::open_boxed(bridge, path, flags)
}
