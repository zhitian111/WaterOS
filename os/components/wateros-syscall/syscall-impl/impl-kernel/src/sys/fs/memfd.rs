//! `memfd_create(2)`：匿名、可 seek、可 mmap 并支持 sealing 的内存文件。
//!
//! 数据和 seals 属于 memfd inode；顺序 offset 与 `O_APPEND` 属于打开文件描述，
//! 因而 `dup`/`fork` 正确共享 offset。prepared-read 只在用户复制成功后推进 offset。

extern crate alloc;

use alloc::{boxed::Box, sync::{Arc, Weak}, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

use api_v0::{ErrNo, SyscallArgs, UserRet};
use spin::Mutex;
use vfs::api::{
    VfsCopyProgress, VfsError, VfsFileContentIdentity, VfsIoHandle, VfsMetadata,
    VfsNodeType, VfsOpenDescriptionState, VfsPreparedRead, VfsReadFinish, VfsReadLease,
    VfsReadReservation, VfsResult, VfsSeekWhence,
};

use crate::{user_copy::copy_user_path_cstr, vfs_util::vfs_error_to_errno};

const MFD_CLOEXEC : usize = 0x0001;
const MFD_ALLOW_SEALING : usize = 0x0002;
const MFD_HUGETLB : usize = 0x0004;
const MFD_NOEXEC_SEAL : usize = 0x0008;
const MFD_EXEC : usize = 0x0010;
const MFD_HUGE_MASK : usize = 0x3f << 26;
const MFD_KNOWN : usize = MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_HUGETLB |
                          MFD_NOEXEC_SEAL | MFD_EXEC | MFD_HUGE_MASK;

pub(crate) const F_SEAL_SEAL : u32 = 0x0001;
pub(crate) const F_SEAL_SHRINK : u32 = 0x0002;
pub(crate) const F_SEAL_GROW : u32 = 0x0004;
pub(crate) const F_SEAL_WRITE : u32 = 0x0008;
pub(crate) const F_SEAL_FUTURE_WRITE : u32 = 0x0010;
pub(crate) const F_SEAL_EXEC : u32 = 0x0020;
const F_SEAL_MASK : u32 = F_SEAL_SEAL | F_SEAL_SHRINK | F_SEAL_GROW |
                          F_SEAL_WRITE | F_SEAL_FUTURE_WRITE | F_SEAL_EXEC;

const FD_CLOEXEC : usize = 1;
const O_APPEND : u32 = 0o0002000;
const O_NONBLOCK : u32 = 0o0004000;
const POLLIN : i16 = 0x001;
const POLLOUT : i16 = 0x004;
const MEMFD_NAME_MAX : usize = 250;

static NEXT_MEMFD_INODE : AtomicU64 = AtomicU64::new(1);
static NEXT_FLOCK_OWNER : AtomicU64 = AtomicU64::new(1);

struct MemFdInner {
    data : Vec<u8>,
    seals : u32,
    writable_shared_mappings : usize,
}

struct MemFdState {
    inner : Mutex<MemFdInner>,
    inode : u64,
    uid : u32,
    gid : u32,
    version : Arc<AtomicU64>,
}

impl MemFdState {
    fn mark_changed(&self) { self.version.fetch_add(1, Ordering::AcqRel); }

    fn write_at(&self, offset : usize, bytes : &[u8]) -> VfsResult<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let end = offset.checked_add(bytes.len()).ok_or(VfsError::NoSpace)?;
        let mut inner = self.inner.lock();
        if inner.seals & F_SEAL_WRITE != 0 {
            return Err(VfsError::OperationNotPermitted);
        }
        if end > inner.data.len() && inner.seals & F_SEAL_GROW != 0 {
            return Err(VfsError::OperationNotPermitted);
        }
        if end > inner.data.len() {
            let additional = end - inner.data.len();
            inner.data.try_reserve_exact(additional).map_err(|_| VfsError::NoMemory)?;
            inner.data.resize(end, 0);
        }
        inner.data[offset..end].copy_from_slice(bytes);
        drop(inner);
        self.mark_changed();
        Ok(bytes.len())
    }

    fn truncate(&self, len : usize) -> VfsResult<()> {
        let mut inner = self.inner.lock();
        let current = inner.data.len();
        if inner.seals & F_SEAL_WRITE != 0 ||
           (len < current && inner.seals & F_SEAL_SHRINK != 0) ||
           (len > current && inner.seals & F_SEAL_GROW != 0)
        {
            return Err(VfsError::OperationNotPermitted);
        }
        if len > current {
            inner.data.try_reserve_exact(len - current).map_err(|_| VfsError::NoMemory)?;
        }
        inner.data.resize(len, 0);
        drop(inner);
        self.mark_changed();
        Ok(())
    }

    fn add_seals(&self, requested : u32) -> Result<usize, ErrNo> {
        if requested & !F_SEAL_MASK != 0 {
            return Err(ErrNo::EINVAL);
        }
        let mut inner = self.inner.lock();
        let added = requested & !inner.seals;
        if added == 0 {
            return Ok(0);
        }
        if inner.seals & F_SEAL_SEAL != 0 {
            return Err(ErrNo::EPERM);
        }
        if added & F_SEAL_WRITE != 0 && inner.writable_shared_mappings != 0 {
            return Err(ErrNo::EBUSY);
        }
        inner.seals |= requested;
        Ok(0)
    }

    fn begin_mapping(self : &Arc<Self>,
                     shared : bool,
                     writable : bool,
                     executable : bool)
                     -> VfsResult<Option<Arc<MemFdMappingLease>>> {
        let mut inner = self.inner.lock();
        if executable && inner.seals & F_SEAL_EXEC != 0 {
            return Err(VfsError::AccessDenied);
        }
        if shared && writable {
            if inner.seals & (F_SEAL_WRITE | F_SEAL_FUTURE_WRITE) != 0 {
                return Err(VfsError::AccessDenied);
            }
            inner.writable_shared_mappings = inner.writable_shared_mappings
                                                    .checked_add(1)
                                                    .ok_or(VfsError::NoSpace)?;
            return Ok(Some(Arc::new(MemFdMappingLease { state : Arc::downgrade(self) })));
        }
        Ok(None)
    }
}

/// loader 持有该 lease，确保存在可写共享 VMA 时不能增加 `F_SEAL_WRITE`。
pub(crate) struct MemFdMappingLease {
    state : Weak<MemFdState>,
}

impl Drop for MemFdMappingLease {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let mut inner = state.inner.lock();
            inner.writable_shared_mappings = inner.writable_shared_mappings.saturating_sub(1);
        }
    }
}

pub(crate) struct MemFdHandle {
    state : Arc<MemFdState>,
    description : Arc<VfsOpenDescriptionState>,
    flock_owner : u64,
}

impl MemFdHandle {
    fn new(initial_seals : u32, uid : u32, gid : u32) -> Self {
        Self { state : Arc::new(MemFdState {
                   inner : Mutex::new(MemFdInner { data : Vec::new(),
                                                   seals : initial_seals,
                                                   writable_shared_mappings : 0 }),
                   inode : NEXT_MEMFD_INODE.fetch_add(1, Ordering::Relaxed),
                   uid,
                   gid,
                   version : Arc::new(AtomicU64::new(1)),
               }),
               description : Arc::new(VfsOpenDescriptionState::new(0, 0)),
               flock_owner : NEXT_FLOCK_OWNER.fetch_add(1, Ordering::Relaxed) }
    }

    fn write_at_inner(&self, offset : u64, bytes : &[u8]) -> VfsResult<usize> {
        let offset = usize::try_from(offset).map_err(|_| VfsError::NoSpace)?;
        self.state.write_at(offset, bytes)
    }
}

impl VfsIoHandle for MemFdHandle {
    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(Box::new(MemFdPreparedRead { state : self.state.clone(),
                                        description : self.description.clone(),
                                        max_len }))
    }

    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> {
        let prepared = self.prepare_read(buf.len())?;
        let lease = prepared.acquire()?;
        let len = lease.bytes().len();
        buf[..len].copy_from_slice(lease.bytes());
        match lease.finish(VfsCopyProgress { copied : len, complete : true })? {
            VfsReadFinish::Bytes(copied) => Ok(copied),
            VfsReadFinish::Fault => Err(VfsError::Io),
        }
    }

    fn write(&mut self, buf : &[u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut reservation = self.description.begin_read()?;
        if self.description.status_flags() & O_APPEND != 0 {
            let end = self.state.inner.lock().data.len() as u64;
            reservation = self.description.retarget_read(reservation, end)?;
        }
        let result = self.write_at_inner(reservation.offset(), buf);
        match result {
            Ok(written) => {
                self.description.finish_read(reservation, written, written)?;
                Ok(written)
            }
            Err(error) => {
                let _ = self.description.cancel_read(reservation);
                Err(error)
            }
        }
    }

    fn read_at(&mut self, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        let offset = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        let inner = self.state.inner.lock();
        if offset >= inner.data.len() {
            return Ok(0);
        }
        let len = buf.len().min(inner.data.len() - offset);
        buf[..len].copy_from_slice(&inner.data[offset..offset + len]);
        Ok(len)
    }

    fn write_at(&mut self, offset : u64, buf : &[u8]) -> VfsResult<usize> {
        self.write_at_inner(offset, buf)
    }

    fn truncate(&mut self, len : u64) -> VfsResult<()> {
        let len = usize::try_from(len).map_err(|_| VfsError::NoSpace)?;
        self.state.truncate(len)
    }

    fn seek(&mut self, offset : i64, whence : VfsSeekWhence) -> VfsResult<u64> {
        match whence {
            VfsSeekWhence::Set if offset >= 0 => {
                self.description.set_offset_if_idle(offset as u64)
            }
            VfsSeekWhence::Cur => self.description.add_signed_offset_if_idle(offset),
            VfsSeekWhence::End => {
                let size = self.state.inner.lock().data.len() as u64;
                let next = if offset < 0 {
                    size.checked_sub(offset.unsigned_abs())
                } else {
                    size.checked_add(offset as u64)
                }.ok_or(VfsError::InvalidPath)?;
                self.description.set_offset_if_idle(next)
            }
            _ => Err(VfsError::InvalidPath),
        }
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(VfsMetadata { node_type : VfsNodeType::File,
                         size : self.state.inner.lock().data.len() as u64,
                         mode : 0o777,
                         device_major : 0,
                         device_minor : 0,
                         inode : self.state.inode,
                         mount_id : 0xffff_fffe,
                         nlink : 0,
                         uid : self.state.uid,
                         gid : self.state.gid })
    }

    fn file_content_identity(&self) -> Option<VfsFileContentIdentity> {
        Some(VfsFileContentIdentity::new(1,
                                         0xffff_fffe,
                                         self.state.inode,
                                         self.state.version.clone()))
    }

    fn flock_owner_id(&self) -> Option<u64> { Some(self.flock_owner) }
    fn flush(&mut self) -> VfsResult<()> { Ok(()) }
    fn open_accmode(&self) -> u32 { 2 }
    fn open_status_flags(&self) -> u32 { self.description.status_flags() }
    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        self.description.set_status_flags(flags & (O_APPEND | O_NONBLOCK));
        Ok(())
    }
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self { state : self.state.clone(),
                           description : self.description.clone(),
                           flock_owner : self.flock_owner }))
    }
    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
        Ok(events & (POLLIN | POLLOUT))
    }
}

struct MemFdPreparedRead {
    state : Arc<MemFdState>,
    description : Arc<VfsOpenDescriptionState>,
    max_len : usize,
}

impl VfsPreparedRead for MemFdPreparedRead {
    fn acquire(self : Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        let reservation = self.description.begin_read()?;
        let offset = usize::try_from(reservation.offset()).map_err(|_| VfsError::Io)?;
        let inner = self.state.inner.lock();
        let len = inner.data.len().saturating_sub(offset).min(self.max_len);
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(len).is_err() {
            drop(inner);
            let _ = self.description.cancel_read(reservation);
            return Err(VfsError::NoMemory);
        }
        bytes.extend_from_slice(&inner.data[offset..offset + len]);
        drop(inner);
        Ok(Box::new(MemFdReadLease { description : self.description,
                                     reservation : Some(reservation),
                                     bytes }))
    }
}

struct MemFdReadLease {
    description : Arc<VfsOpenDescriptionState>,
    reservation : Option<VfsReadReservation>,
    bytes : Vec<u8>,
}

impl VfsReadLease for MemFdReadLease {
    fn bytes(&self) -> &[u8] { self.bytes.as_slice() }

    fn finish(mut self : Box<Self>, progress : VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied > self.bytes.len() {
            return Err(VfsError::Io);
        }
        let reservation = self.reservation.take().ok_or(VfsError::Io)?;
        self.description.finish_read(reservation, progress.copied, self.bytes.len())?;
        if progress.copied == 0 && !progress.complete {
            Ok(VfsReadFinish::Fault)
        } else {
            Ok(VfsReadFinish::Bytes(progress.copied))
        }
    }
}

impl Drop for MemFdReadLease {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            let _ = self.description.cancel_read(reservation);
        }
    }
}

pub(crate) fn prepare_mapping(handle : &(dyn VfsIoHandle + '_),
                              shared : bool,
                              writable : bool,
                              executable : bool)
                              -> VfsResult<Option<Arc<MemFdMappingLease>>> {
    let Some(memfd) = handle.as_any().downcast_ref::<MemFdHandle>() else {
        return Ok(None);
    };
    memfd.state.begin_mapping(shared, writable, executable)
}

pub(crate) fn fcntl_add_seals(fd : usize, seals : u32) -> Result<usize, ErrNo> {
    let mut result = None;
    match vfs::fd::with_current_io(fd, |handle| {
              let memfd = handle.as_any().downcast_ref::<MemFdHandle>()
                                .ok_or(VfsError::Unsupported)?;
              result = Some(memfd.state.add_seals(seals));
              Ok(())
          }) {
        Ok(()) => result.unwrap_or(Err(ErrNo::EINVAL)),
        Err(VfsError::BadFd) => Err(ErrNo::EBADF),
        Err(_) => Err(ErrNo::EINVAL),
    }
}

pub(crate) fn fcntl_get_seals(fd : usize) -> Result<usize, ErrNo> {
    vfs::fd::with_current_io(fd, |handle| {
        let memfd = handle.as_any().downcast_ref::<MemFdHandle>()
                          .ok_or(VfsError::Unsupported)?;
        Ok(memfd.state.inner.lock().seals as usize)
    }).map_err(|error| match error {
          VfsError::BadFd => ErrNo::EBADF,
          _ => ErrNo::EINVAL,
      })
}

pub(crate) fn sys_memfd_create(args : SyscallArgs) -> UserRet {
    let name_ptr = args.arg(0);
    let flags = args.arg(1);
    if flags & !MFD_KNOWN != 0 || flags & MFD_HUGE_MASK != 0 && flags & MFD_HUGETLB == 0 ||
       flags & MFD_NOEXEC_SEAL != 0 && flags & MFD_EXEC != 0
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & MFD_HUGETLB != 0 {
        return UserRet::from_error(ErrNo::EOPNOTSUPP);
    }
    if let Err(error) = copy_user_path_cstr(name_ptr, MEMFD_NAME_MAX) {
        return UserRet::from_error(error);
    }

    let credentials = cred::current_credentials();
    let mut initial_seals = if flags & MFD_ALLOW_SEALING != 0 {
        0
    } else {
        F_SEAL_SEAL
    };
    if flags & MFD_NOEXEC_SEAL != 0 {
        initial_seals |= F_SEAL_EXEC;
    }
    let fd = match vfs::fd::alloc_fd(Box::new(MemFdHandle::new(initial_seals,
                                                               credentials.effective_uid.0,
                                                               credentials.effective_gid.0))) {
        Ok(fd) => fd,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    if flags & MFD_CLOEXEC != 0 {
        if let Err(error) = vfs::fd::set_fd_flags(fd, FD_CLOEXEC) {
            let _ = vfs::fd::close_fd(fd);
            return UserRet::from_error(vfs_error_to_errno(error));
        }
    }
    UserRet::from_success(fd)
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    assert_eq!(F_SEAL_MASK, 0x3f);
    assert_eq!(MEMFD_NAME_MAX, 250);
}
