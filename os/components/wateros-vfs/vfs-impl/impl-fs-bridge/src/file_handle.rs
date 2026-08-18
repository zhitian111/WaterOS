//! ext4 根卷小文件缓冲句柄；普通 `open` 主路径已改由 [`super::paged_handle`] 处理。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::mount_table::{resolve_route, FsRoute};
use crate::{replace_file_contents, FsBridge};
use api_v0::*;
use spin::Mutex;

use crate::read_lease::{try_zeroed, ReservationGuard, StagedReadLease};

const S_IFMT : u16 = 0o170000;
const S_IFIFO : u16 = 0o010000;
const S_IFCHR : u16 = 0o020000;

static NEXT_FLOCK_OWNER_ID : AtomicU64 = AtomicU64::new(1);

fn open_accmode(flags : VfsOpenFlags) -> u32 {
    let read = flags.contains(VfsOpenFlags::READ);
    let write = flags.contains(VfsOpenFlags::WRITE);
    match (read, write) {
        (false, true) => 1,
        (true, true) => 2,
        _ => 0,
    }
}

/// 已打开的根卷普通文件（小文件：全文缓冲于内存）。
#[derive(Clone)]
// 本结构代码由AI完成
pub struct BufferedFileHandle {
    /// 绝对路径，用于回写和诊断。
    path : String,
    /// 小文件的共享内容缓冲；由 Mutex 保护。
    data : Arc<Mutex<Vec<u8>>>,
    description : Arc<VfsOpenDescriptionState>,
    meta : VfsMetadata,
    writable : bool,
    accmode : u32,
    dirty : bool,
    flock_owner_id : u64,
}

/// 与历史命名兼容的别名。
pub type RootFileHandle = BufferedFileHandle;

impl BufferedFileHandle {
    // 本方法代码由AI完成
    pub(crate) fn open(bridge : &FsBridge, path : String, flags : VfsOpenFlags) -> VfsResult<Self> {
        let want_read = flags.contains(VfsOpenFlags::READ);
        let want_write = flags.contains(VfsOpenFlags::WRITE);
        if !want_read && !want_write {
            return Err(VfsError::Unsupported);
        }

        let exists = bridge.exists(path.as_str())?;
        if !exists {
            if !flags.contains(VfsOpenFlags::CREATE) {
                return Err(VfsError::NotFound);
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
        let mut status_flags = 0u32;
        if flags.contains(VfsOpenFlags::APPEND) {
            offset = data.len() as u64;
            status_flags |= 0o2000;
        }

        let mut meta = meta;
        meta.size = data.len() as u64;

        let accmode = open_accmode(flags);

        Ok(Self { path,
                  data : Arc::new(Mutex::new(data)),
                  description : Arc::new(VfsOpenDescriptionState::new(offset, status_flags)),
                  meta,
                  writable : want_write,
                  accmode,
                  dirty,
                  flock_owner_id : NEXT_FLOCK_OWNER_ID.fetch_add(1, Ordering::Relaxed) })
    }

    // 本方法代码由AI完成
    #[allow(dead_code)]
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
        let data = self.data.lock();
        let mut snapshot = Vec::new();
        snapshot.try_reserve_exact(data.len())
                .map_err(|_| VfsError::NoMemory)?;
        snapshot.extend_from_slice(data.as_slice());
        drop(data);
        replace_file_contents(self.path.as_str(), snapshot.as_slice())?;
        self.dirty = false;
        self.meta.size = snapshot.len() as u64;
        Ok(())
    }
}

impl VfsIoHandle for BufferedFileHandle {
    fn resource_kind(&self) -> VfsResourceKind { VfsResourceKind::Regular }

    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        let reservation = ReservationGuard::begin(self.description
                                                      .clone())?;
        Ok(Box::new(BufferedPreparedRead { reservation,
                                           data : self.data.clone(),
                                           max_len }))
    }

    // 本方法代码由AI完成
    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut reservation = ReservationGuard::begin(self.description
                                                          .clone())?;
        let offset = reservation.offset();
        let off = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        let data = self.data.lock();
        if off >= data.len() {
            reservation.commit(0, 0)?;
            return Ok(0);
        }
        let n = buf.len()
                   .min(data.len() - off);
        buf[..n].copy_from_slice(&data[off..off + n]);
        drop(data);
        reservation.commit(n, n)?;
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
        const O_APPEND : u32 = 0o2000;
        let mut reservation = ReservationGuard::begin(self.description
                                                          .clone())?;
        if self.description
               .status_flags() &
           O_APPEND !=
           0
        {
            let end = self.data
                          .lock()
                          .len() as u64;
            reservation.retarget(end)?;
        }
        let off = usize::try_from(reservation.offset()).map_err(|_| VfsError::Io)?;
        let mut data = self.data.lock();
        let end = off.checked_add(buf.len())
                     .ok_or(VfsError::Io)?;
        if end > data.len() {
            data.resize(end, 0);
        }
        data[off..end].copy_from_slice(buf);
        self.meta.size = data.len() as u64;
        drop(data);
        reservation.commit(buf.len(), buf.len())?;
        self.dirty = true;
        const O_SYNC : u32 = 0o4_010_000;
        if self.description
               .status_flags() &
           O_SYNC !=
           0
        {
            self.sync_dirty()?;
        }
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
        m.size = self.data
                     .lock()
                     .len() as u64;
        Ok(m)
    }

    // 本方法代码由AI完成
    fn backing_path(&self) -> Option<&str> { Some(self.path.as_str()) }

    // 本方法代码由AI完成
    fn flock_owner_id(&self) -> Option<u64> { Some(self.flock_owner_id) }

    // 本方法代码由AI完成
    fn open_accmode(&self) -> u32 { self.accmode }

    // 本方法代码由AI完成
    fn read_at(&mut self, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let off = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        let data = self.data.lock();
        if off >= data.len() {
            return Ok(0);
        }
        let n = buf.len()
                   .min(data.len() - off);
        buf[..n].copy_from_slice(&data[off..off + n]);
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
        let mut data = self.data.lock();
        if end > data.len() {
            data.resize(end, 0);
        }
        data[off..end].copy_from_slice(buf);
        self.meta.size = data.len() as u64;
        drop(data);
        self.dirty = true;
        const O_SYNC : u32 = 0o4_010_000;
        if self.description
               .status_flags() &
           O_SYNC !=
           0
        {
            self.sync_dirty()?;
        }
        Ok(buf.len())
    }

    // 本方法代码由AI完成
    fn truncate(&mut self, len : u64) -> VfsResult<()> {
        if !self.writable {
            return Err(VfsError::Unsupported);
        }
        let mut reservation = ReservationGuard::begin(self.description
                                                          .clone())?;
        let len = usize::try_from(len).map_err(|_| VfsError::InvalidPath)?;
        self.data
            .lock()
            .resize(len, 0);
        self.meta.size = len as u64;
        reservation.commit_at(reservation.offset()
                                         .min(self.meta.size))?;
        self.dirty = true;
        Ok(())
    }

    // 本方法代码由AI完成
    fn seek(&mut self, offset : i64, whence : VfsSeekWhence) -> VfsResult<u64> {
        log::info!("[buffered_handle] seek path={} whence={whence:?} offset={offset} cur={}",
                   self.path,
                   self.description
                       .offset(),);
        let result = (|| {
            let new_off = match whence {
                VfsSeekWhence::Set => {
                    if offset < 0 {
                        return Err(VfsError::InvalidPath);
                    }
                    offset as u64
                }
                VfsSeekWhence::Cur => {
                    return self.description
                               .add_signed_offset_if_idle(offset);
                }
                VfsSeekWhence::End => {
                    let base = self.data
                                   .lock()
                                   .len() as u64;
                    if offset < 0 {
                        base.checked_sub((-offset) as u64)
                            .ok_or(VfsError::InvalidPath)?
                    } else {
                        base.checked_add(offset as u64)
                            .ok_or(VfsError::InvalidPath)?
                    }
                }
            };
            self.description
                .set_offset_if_idle(new_off)
        })();
        log::info!("[buffered_handle] seek done path={} result={result:?}",
                   self.path,);
        result
    }

    // 本方法代码由AI完成
    fn flush(&mut self) -> VfsResult<()> { self.sync_dirty() }

    fn open_status_flags(&self) -> u32 {
        self.description
            .status_flags()
    }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        const O_APPEND : u32 = 0o2000;
        const O_NONBLOCK : u32 = 0o4000;
        // asm-generic 的 O_SYNC 包含 O_DSYNC 位，因此该掩码保留 open 时启用的同步写模式。
        const O_SYNC : u32 = 0o4_010_000;
        self.description
            .set_status_flags(flags & (O_APPEND | O_NONBLOCK | O_SYNC));
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

struct BufferedPreparedRead {
    reservation : ReservationGuard,
    data : Arc<Mutex<Vec<u8>>>,
    max_len : usize,
}

impl VfsPreparedRead for BufferedPreparedRead {
    fn acquire(self: Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        let start = usize::try_from(self.reservation
                                        .offset()).map_err(|_| VfsError::Io)?;
        let data = self.data.lock();
        let len = data.len()
                      .saturating_sub(start)
                      .min(self.max_len);
        let mut staged = try_zeroed(len)?;
        if len != 0 {
            staged.copy_from_slice(&data[start..start + len]);
        }
        drop(data);
        let Self { reservation, .. } = *self;
        Ok(Box::new(StagedReadLease::new(reservation, staged)))
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
            FsRoute::PseudoSys { rel, identity } => {
                return super::proc_handle::open_sys(rel, abs.clone(), flags, identity);
            }
            FsRoute::PseudoSecurity { rel, .. }
                if rel == "/" && flags.contains(VfsOpenFlags::DIRECTORY) =>
            {
                return super::dir_handle::DirectoryHandle::open(self, abs);
            }
            FsRoute::PseudoSecurity { .. } => return Err(VfsError::NotFound),
            _ => {}
        }
        match abs.as_str() {
            "/dev/null" => {
                return Ok(Box::new(impl_fd_session::NullDeviceHandle::new(
                    open_accmode(flags),
                    impl_fd_session::devfs_node_inode("/dev/null"),
                )));
            }
            "/dev/zero" => {
                return Ok(Box::new(impl_fd_session::ZeroDeviceHandle::new(
                    open_accmode(flags),
                    impl_fd_session::devfs_node_inode("/dev/zero"),
                )));
            }
            "/dev/random" | "/dev/urandom" => {
                return Ok(Box::new(impl_fd_session::UrandomDeviceHandle::new(
                    open_accmode(flags),
                    impl_fd_session::devfs_node_inode(abs.as_str()),
                )));
            }
            "/dev/cpu_dma_latency" => {
                return Ok(Box::new(impl_fd_session::CpuDmaLatencyDeviceHandle::new(
                    open_accmode(flags),
                    impl_fd_session::devfs_node_inode("/dev/cpu_dma_latency"),
                )));
            }
            _ => {}
        }
        if let Some(opened) =
            impl_fd_session::open_special_device(abs.as_str(),
                                                 open_accmode(flags),
                                                 flags.contains(VfsOpenFlags::NONBLOCK))
        {
            return opened;
        }
        if let Ok(dev) = fs::devfs::active_impl::lookup_character_device(abs.as_str()) {
            return Ok(Box::new(impl_fd_session::CharDevHandle::from_devfs_path(dev,
                                                                               abs.as_str(),
                                                                               open_accmode(flags))));
        }
        if let Ok(meta) = self.metadata(abs.as_str()) {
            if meta.node_type == VfsNodeType::Symlink {
                // syscall 层只在 `O_PATH|O_NOFOLLOW` 时把符号链接路径原样传入，
                // 此时应打开链接节点本身（systemd-tmpfiles 依赖）。
                return Ok(Box::new(super::symlink_handle::SymlinkPathHandle::new(abs.clone(),
                                                                                 meta)));
            }
            if flags.contains(VfsOpenFlags::DIRECTORY) && meta.node_type != VfsNodeType::Directory {
                return Err(VfsError::NotDirectory);
            }
            if meta.node_type == VfsNodeType::Special && meta.mode & S_IFMT == S_IFIFO {
                return impl_fd_session::open_named_pipe(meta, flags);
            }
            if meta.node_type == VfsNodeType::Special && meta.mode & S_IFMT == S_IFCHR {
                return Ok(Box::new(impl_fd_session::NullDeviceHandle::new(open_accmode(flags),
                                                                          meta.inode)));
            }
            if meta.node_type == VfsNodeType::Directory && !flags.contains(VfsOpenFlags::WRITE) {
                return super::dir_handle::DirectoryHandle::open(self, abs);
            }
        }
        if flags.contains(VfsOpenFlags::DIRECTORY) {
            return super::dir_handle::DirectoryHandle::open(self, abs);
        }
        match resolve_route(abs.as_str())? {
            FsRoute::AuxRw { readonly: true, .. } if flags.contains(VfsOpenFlags::WRITE) => {
                Err(VfsError::ReadOnlyFs)
            }
            FsRoute::AuxRo { .. } => Ok(Box::new(BufferedFileHandle::open(self, abs, flags)?)),
            _ => super::paged_handle::open_file(self, abs, flags),
        }
    }
}
