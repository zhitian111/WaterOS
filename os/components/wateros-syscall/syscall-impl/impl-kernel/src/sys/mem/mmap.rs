//! `mmap` / `munmap` / `mprotect`：经 `user_aspace` 句柄拼合 `MmapOps`。

//! 本模块代码由AI完成
extern crate alloc;

use alloc::boxed::Box;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use mm::api::addr::PAGE_SIZE;
use mm::api::error::{MmError, MmResult};
use mm::api::flags::MapFlags;
use mm::api::mmap::{DemandPageLoader, MmapKind, MmapOps};

use crate::mm_util::{
    linux_mmap_flags_to_map_flags, linux_mmap_is_anonymous, linux_mmap_prot_to_perm,
    mm_err_to_errno, require_user_aspace,
};
use crate::vfs_util::vfs_error_to_errno;
use vfs::api::{VfsError, VfsIoHandle, VfsNodeType};

struct VfsMmapPageLoader {
    handle : Box<dyn VfsIoHandle>,
    file_size : usize,
}

impl DemandPageLoader for VfsMmapPageLoader {
    fn duplicate_box(&self) -> MmResult<Box<dyn DemandPageLoader>> {
        let handle = self.handle
                         .duplicate()
                         .map_err(|_| MmError::AccessViolation)?;
        Ok(Box::new(Self { handle,
                           file_size : self.file_size }))
    }

    fn load_page(&mut self, file_offset : usize, dst : &mut [u8]) -> MmResult<()> {
        if file_offset >= self.file_size {
            return Ok(());
        }
        let readable = core::cmp::min(dst.len(), self.file_size - file_offset);
        let mut done = 0usize;
        while done < readable {
            let off = file_offset.checked_add(done)
                                 .ok_or(MmError::InvalidAddress)?;
            let n = self.handle
                        .read_at(off as u64, &mut dst[done..readable])
                        .map_err(|_| MmError::AccessViolation)?;
            if n == 0 {
                break;
            }
            done = done.checked_add(n)
                       .ok_or(MmError::InvalidAddress)?;
        }
        Ok(())
    }

    fn write_page(&mut self, file_offset : usize, src : &[u8]) -> MmResult<()> {
        if file_offset >= self.file_size {
            return Ok(());
        }
        let writable = core::cmp::min(src.len(), self.file_size - file_offset);
        let mut done = 0usize;
        while done < writable {
            let off = file_offset.checked_add(done)
                                 .ok_or(MmError::InvalidAddress)?;
            let n = self.handle
                        .write_at(off as u64, &src[done..writable])
                        .map_err(|_| MmError::AccessViolation)?;
            if n == 0 {
                return Err(MmError::AccessViolation);
            }
            done = done.checked_add(n)
                       .ok_or(MmError::InvalidAddress)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> MmResult<()> {
        self.handle.flush().map_err(|_| MmError::AccessViolation)
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_mmap(args : SyscallArgs) -> UserRet {
    let handle = match require_user_aspace("mmap") {
        Ok(handle) => handle,
        Err(e) => return UserRet::from_error(e),
    };
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::MmapRequest;
    use mm::frame_alloctor::GlobalPhysFrameAllocator;

    let addr = args.arg(0);
    let len = args.arg(1);
    let prot = args.arg(2) as i32;
    let flags = args.arg(3) as u32;
    let fd_arg = args.arg(4);
    let offset = args.arg(5);

    if len == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let perm = linux_mmap_prot_to_perm(prot);
    let mf = linux_mmap_flags_to_map_flags(flags);
    let addr_hint = if addr != 0 {
        Some(VirtAddr(addr))
    } else {
        None
    };

    let (kind, file_fd, file_size) = if linux_mmap_is_anonymous(flags) {
        if !mf.contains(MapFlags::PRIVATE) && !mf.contains(MapFlags::SHARED) {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        (MmapKind::Anonymous, None, 0usize)
    } else {
        if !mf.contains(MapFlags::SHARED) && !mf.contains(MapFlags::PRIVATE) {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        let fd = fd_arg as isize;
        if fd < 0 {
            return UserRet::from_error(ErrNo::EBADF);
        }
        if offset % PAGE_SIZE != 0 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        const O_ACCMODE : u32 = 3;
        const O_WRONLY : u32 = 1;
        let accmode = match vfs::fd::with_current_io(fd as usize, |handle| {
            Ok(handle.open_accmode())
        }) {
            Ok(mode) => mode,
            Err(_) => return UserRet::from_error(ErrNo::EBADF),
        };
        if accmode & O_ACCMODE == O_WRONLY {
            return UserRet::from_error(ErrNo::EACCES);
        }
        let size = match file_size_for_mmap(fd as usize) {
            Ok(size) => size,
            Err(e) => return UserRet::from_error(e),
        };
        (MmapKind::File { fd : fd as usize,
                          offset },
         Some(fd as usize),
         size)
    };

    let req = MmapRequest { addr_hint,
                            len,
                            prot : perm,
                            flags : mf,
                            kind };

    match file_fd {
        None => match mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
                  let mut alloc = GlobalPhysFrameAllocator;
                  let base = MmapOps::mmap(aspace, &mut alloc, req, None)?;
                  Ok(base.0)
              }) {
            Ok(base) => UserRet::from_success(base),
            Err(e) => {
                let errno = mm_err_to_errno(e);
                UserRet::from_error(errno)
            }
        },
        Some(fd) => {
            let loader = match mmap_page_loader(fd, file_size) {
                Ok(loader) => loader,
                Err(e) => return UserRet::from_error(e),
            };
            // Writable shared mappings need stable frames across fork. Read-only shared mappings
            // can retain file-backed lazy faults without changing observable sharing semantics.
            let eager_shared = mf.contains(MapFlags::SHARED) && perm.writable();
            match mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
                      let mut alloc = GlobalPhysFrameAllocator;
                      let base = if eager_shared {
                          MmapOps::mmap_file_shared(aspace, &mut alloc, req, loader)?
                      } else {
                          MmapOps::mmap_file_lazy(aspace, &mut alloc, req, file_size, loader)?
                      };
                      Ok(base.0)
                  }) {
                Ok(base) => UserRet::from_success(base),
                Err(e) => {
                    let errno = mm_err_to_errno(e);
                    UserRet::from_error(errno)
                }
            }
        }
    }
}

fn mmap_page_loader(fd : usize, file_size : usize) -> Result<Box<dyn DemandPageLoader>, ErrNo> {
    let handle = vfs::fd::with_current_io(fd, |handle| handle.duplicate())
        .map_err(vfs_error_to_errno)?;
    Ok(Box::new(VfsMmapPageLoader { handle,
                                     file_size }))
}

fn file_size_for_mmap(fd : usize) -> Result<usize, ErrNo> {
    let meta = vfs::fd::with_current_io(fd, |handle| {
                   let meta = handle.metadata()?;
                   if meta.node_type != VfsNodeType::File {
                       return Err(VfsError::NotAFile);
                   }
                   Ok(meta)
               }).map_err(vfs_error_to_errno)?;
    usize::try_from(meta.size).map_err(|_| ErrNo::EINVAL)
}

// 本方法代码由AI完成
pub(crate) fn sys_munmap(args : SyscallArgs) -> UserRet {
    let handle = match require_user_aspace("munmap") {
        Ok(handle) => handle,
        Err(e) => return UserRet::from_error(e),
    };
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::MmapOps;
    use mm::frame_alloctor::GlobalPhysFrameAllocator;
    let addr = args.arg(0);
    let len = args.arg(1);
    match mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
              let mut alloc = GlobalPhysFrameAllocator;
              MmapOps::munmap(aspace, &mut alloc, VirtAddr(addr), len)
          }) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_msync(args : SyscallArgs) -> UserRet {
    use mm::api::addr::{VirtAddr, PAGE_SIZE};

    const MS_ASYNC : usize = 0x1;
    const MS_INVALIDATE : usize = 0x2;
    const MS_SYNC : usize = 0x4;
    const MS_KNOWN : usize = MS_ASYNC | MS_INVALIDATE | MS_SYNC;

    let addr = args.arg(0);
    let len = args.arg(1);
    let flags = args.arg(2);

    if addr % PAGE_SIZE != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & !MS_KNOWN != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & MS_ASYNC != 0 && flags & MS_SYNC != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let handle = match require_user_aspace("msync") {
        Ok(handle) => handle,
        Err(e) => return UserRet::from_error(e),
    };
    match mm::user_aspace::with_user_aspace_mut(handle, |aspace| {
              MmapOps::msync(aspace, VirtAddr(addr), len)
          }) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_mprotect(args : SyscallArgs) -> UserRet {
    let handle = match require_user_aspace("mprotect") {
        Ok(handle) => handle,
        Err(e) => return UserRet::from_error(e),
    };
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::MmapOps;
    let addr = args.arg(0);
    let len = args.arg(1);
    let prot = args.arg(2) as i32;
    let perm = linux_mmap_prot_to_perm(prot);
    match mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
              MmapOps::mprotect(aspace, VirtAddr(addr), len, perm)
          }) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_mremap(args : SyscallArgs) -> UserRet {
    let handle = match require_user_aspace("mremap") {
        Ok(handle) => handle,
        Err(e) => return UserRet::from_error(e),
    };
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::MmapOps;
    use mm::frame_alloctor::GlobalPhysFrameAllocator;

    let old_addr = args.arg(0);
    let old_size = args.arg(1);
    let new_size = args.arg(2);
    let flags = args.arg(3);
    let new_address = args.arg(4);

    match mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
              let mut alloc = GlobalPhysFrameAllocator;
              let base = MmapOps::mremap(aspace,
                                         &mut alloc,
                                         VirtAddr(old_addr),
                                         old_size,
                                         new_size,
                                         flags,
                                         VirtAddr(new_address))?;
              Ok(base.0)
          }) {
        Ok(base) => UserRet::from_success(base),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_madvise(args : SyscallArgs) -> UserRet {
    use mm::api::addr::PAGE_SIZE;

    const MADV_NORMAL : usize = 0;
    const MADV_RANDOM : usize = 1;
    const MADV_SEQUENTIAL : usize = 2;
    const MADV_WILLNEED : usize = 3;
    const MADV_DONTNEED : usize = 4;
    const MADV_FREE : usize = 8;
    const MADV_REMOVE : usize = 9;
    const MADV_DONTFORK : usize = 10;
    const MADV_DOFORK : usize = 11;
    const MADV_MERGEABLE : usize = 12;
    const MADV_UNMERGEABLE : usize = 13;
    const MADV_HUGEPAGE : usize = 14;
    const MADV_NOHUGEPAGE : usize = 15;
    const MADV_DONTDUMP : usize = 16;
    const MADV_DODUMP : usize = 17;
    const MADV_WIPEONFORK : usize = 18;
    const MADV_KEEPONFORK : usize = 19;
    const MADV_COLD : usize = 20;
    const MADV_PAGEOUT : usize = 21;
    const MADV_POPULATE_READ : usize = 22;
    const MADV_POPULATE_WRITE : usize = 23;
    const MADV_DONTNEED_LOCKED : usize = 24;
    const MADV_COLLAPSE : usize = 25;

    let addr = args.arg(0);
    let len = args.arg(1);
    let advice = args.arg(2);

    if addr % PAGE_SIZE != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if addr.checked_add(len)
           .is_none()
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    match advice {
        MADV_DONTNEED | MADV_FREE => {
            let handle = match require_user_aspace("madvise") {
                Ok(h) => h,
                Err(e) => return UserRet::from_error(e),
            };
            match mm::kernel_mm::madvise_range_shared_or_file(handle, addr, len) {
                Ok(true) => return UserRet::from_error(ErrNo::EINVAL),
                Ok(false) => {}
                Err(e) => return UserRet::from_error(mm_err_to_errno(e)),
            }
            match mm::kernel_mm::madvise_discard_pages(handle, addr, len) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(mm_err_to_errno(e)),
            }
        }
        MADV_NORMAL | MADV_RANDOM | MADV_SEQUENTIAL | MADV_WILLNEED |
        MADV_DONTFORK | MADV_DOFORK | MADV_HUGEPAGE | MADV_NOHUGEPAGE | MADV_DONTDUMP |
        MADV_DODUMP | MADV_COLD | MADV_PAGEOUT | MADV_POPULATE_READ | MADV_POPULATE_WRITE |
        MADV_DONTNEED_LOCKED => {
            let handle = match require_user_aspace("madvise") {
                Ok(h) => h,
                Err(e) => return UserRet::from_error(e),
            };
            match mm::kernel_mm::madvise_range_mapped(handle, addr, len) {
                Ok(true) => {
                    log::trace!("[syscall] madvise(nr=28) no-op advice={advice}");
                    UserRet::from_success(0)
                }
                Ok(false) => UserRet::from_error(ErrNo::ENOMEM),
                Err(e) => UserRet::from_error(mm_err_to_errno(e)),
            }
        }
        MADV_REMOVE | MADV_MERGEABLE | MADV_UNMERGEABLE | MADV_WIPEONFORK | MADV_KEEPONFORK |
        MADV_COLLAPSE => {
            log::trace!("[syscall] madvise(nr=28) unsupported advice={advice}");
            UserRet::from_error(ErrNo::EINVAL)
        }
        _ => UserRet::from_error(ErrNo::EINVAL),
    }
}

fn validate_mlock_range(addr : usize, len : usize) -> Result<(), ErrNo> {
    use mm::api::addr::PAGE_SIZE;

    if len == 0 {
        return Err(ErrNo::EINVAL);
    }
    if addr % PAGE_SIZE != 0 {
        return Err(ErrNo::EINVAL);
    }
    addr.checked_add(len)
        .ok_or(ErrNo::EINVAL)?;
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn sys_mlock(args : SyscallArgs) -> UserRet {
    let addr = args.arg(0);
    let len = args.arg(1);
    if validate_mlock_range(addr, len).is_err() {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    log::trace!("[syscall] mlock(nr=149) no-op success");
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_munlock(args : SyscallArgs) -> UserRet {
    let addr = args.arg(0);
    let len = args.arg(1);
    if validate_mlock_range(addr, len).is_err() {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_mlockall(args : SyscallArgs) -> UserRet {
    const MCL_CURRENT : usize = 0x1;
    const MCL_FUTURE : usize = 0x2;
    const MCL_ONFAULT : usize = 0x4;
    const MCL_KNOWN : usize = MCL_CURRENT | MCL_FUTURE | MCL_ONFAULT;

    let flags = args.arg(0);
    if flags & !MCL_KNOWN != 0 || flags == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_munlockall(_args : SyscallArgs) -> UserRet { UserRet::from_success(0) }
