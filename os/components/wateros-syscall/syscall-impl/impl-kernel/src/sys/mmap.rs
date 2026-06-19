//! `mmap` / `munmap` / `mprotect`：经 `user_aspace` 句柄拼合 `MmapOps`。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::mm_util::{
    current_user_aspace_handle, linux_mmap_flags_to_map_flags, linux_mmap_is_anonymous,
    linux_mmap_prot_to_perm, mm_err_to_errno,
};
use crate::vfs_util::vfs_error_to_errno;
use api_v0::unsupported::syscall_unsupported;
use vfs::api::{VfsError, VfsNodeType};

pub(crate) fn sys_mmap(args : SyscallArgs) -> UserRet {
    let Some(handle) = current_user_aspace_handle() else {
        syscall_unsupported("mmap: no user_aspace_ptr");
    };
    use mm::api::addr::{VirtAddr, PAGE_SIZE};
    use mm::api::flags::MapFlags;
    use mm::api::mmap::{MmapKind, MmapOps, MmapRequest};
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
        if !mf.contains(MapFlags::PRIVATE) {
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
        None => match mm::user_aspace::with_user_aspace_mut(handle, |aspace| {
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
            let mut file_errno = None;
            match mm::user_aspace::with_user_aspace_mut(handle, |aspace| {
                      let mut alloc = GlobalPhysFrameAllocator;
                      let base = MmapOps::mmap_file_with_loader(
                                                                aspace,
                                                                &mut alloc,
                                                                req,
                                                                |page_index, page| {
                                                                    if let Err(errno) =
                                                                  load_mmap_file_page(fd,
                                                                                      offset,
                                                                                      len,
                                                                                      file_size,
                                                                                      page_index,
                                                                                      page)
                                                              {
                                                                  file_errno = Some(errno);
                                                                  return Err(
                                                                      mm::api::error::MmError::AccessViolation,
                                                                  );
                                                              }
                                                                    Ok(())
                                                                },
                )?;
                      Ok(base.0)
                  }) {
                Ok(base) => UserRet::from_success(base),
                Err(e) => {
                    let errno = file_errno.unwrap_or_else(|| mm_err_to_errno(e));
                    UserRet::from_error(errno)
                }
            }
        }
    }
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

fn load_mmap_file_page(fd : usize,
                       mmap_offset : usize,
                       mmap_len : usize,
                       file_size : usize,
                       page_index : usize,
                       page : &mut [u8])
                       -> Result<(), ErrNo> {
    use mm::api::addr::PAGE_SIZE;

    let page_offset = page_index.checked_mul(PAGE_SIZE)
                                .ok_or(ErrNo::EINVAL)?;
    let file_offset = mmap_offset.checked_add(page_offset)
                                 .ok_or(ErrNo::EINVAL)?;
    if file_offset >= file_size || page_offset >= mmap_len {
        return Ok(());
    }

    let valid_mmap_bytes = core::cmp::min(PAGE_SIZE, mmap_len - page_offset);
    let readable = core::cmp::min(valid_mmap_bytes,
                                  file_size - file_offset);
    let mut done = 0usize;
    while done < readable {
        let off = file_offset.checked_add(done)
                             .ok_or(ErrNo::EINVAL)?;
        let n = vfs::fd::with_current_io(fd, |handle| {
                    handle.read_at(off as u64, &mut page[done..readable])
                }).map_err(vfs_error_to_errno)?;
        if n == 0 {
            break;
        }
        done = done.checked_add(n)
                   .ok_or(ErrNo::EINVAL)?;
    }
    Ok(())
}

pub(crate) fn sys_munmap(args : SyscallArgs) -> UserRet {
    let Some(handle) = current_user_aspace_handle() else {
        syscall_unsupported("mmap: no user_aspace_ptr");
    };
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::MmapOps;
    use mm::frame_alloctor::GlobalPhysFrameAllocator;
    let addr = args.arg(0);
    let len = args.arg(1);
    match mm::user_aspace::with_user_aspace_mut(handle, |aspace| {
              let mut alloc = GlobalPhysFrameAllocator;
              MmapOps::munmap(aspace, &mut alloc, VirtAddr(addr), len)
          }) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

pub(crate) fn sys_msync(args : SyscallArgs) -> UserRet {
    use mm::api::addr::PAGE_SIZE;

    const MS_ASYNC : usize = 0x1;
    const MS_INVALIDATE : usize = 0x2;
    const MS_SYNC : usize = 0x4;
    const MS_KNOWN : usize = MS_ASYNC | MS_INVALIDATE | MS_SYNC;

    let addr = args.arg(0);
    let _len = args.arg(1);
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

    UserRet::from_success(0)
}

pub(crate) fn sys_mprotect(args : SyscallArgs) -> UserRet {
    let Some(handle) = current_user_aspace_handle() else {
        syscall_unsupported("mmap: no user_aspace_ptr");
    };
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::MmapOps;
    let addr = args.arg(0);
    let len = args.arg(1);
    let prot = args.arg(2) as i32;
    let perm = linux_mmap_prot_to_perm(prot);
    match mm::user_aspace::with_user_aspace_mut(handle, |aspace| {
              MmapOps::mprotect(aspace, VirtAddr(addr), len, perm)
          }) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

pub(crate) fn sys_mremap(args : SyscallArgs) -> UserRet {
    let Some(handle) = current_user_aspace_handle() else {
        syscall_unsupported("mremap: no user_aspace_ptr");
    };
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::MmapOps;
    use mm::frame_alloctor::GlobalPhysFrameAllocator;

    let old_addr = args.arg(0);
    let old_size = args.arg(1);
    let new_size = args.arg(2);
    let flags = args.arg(3);
    let new_address = args.arg(4);

    match mm::user_aspace::with_user_aspace_mut(handle, |aspace| {
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
        MADV_NORMAL | MADV_RANDOM | MADV_SEQUENTIAL | MADV_WILLNEED | MADV_DONTNEED |
        MADV_FREE | MADV_REMOVE | MADV_DONTFORK | MADV_DOFORK | MADV_MERGEABLE |
        MADV_UNMERGEABLE | MADV_HUGEPAGE | MADV_NOHUGEPAGE | MADV_DONTDUMP | MADV_DODUMP |
        MADV_WIPEONFORK | MADV_KEEPONFORK | MADV_COLD | MADV_PAGEOUT | MADV_POPULATE_READ |
        MADV_POPULATE_WRITE | MADV_DONTNEED_LOCKED | MADV_COLLAPSE => UserRet::from_success(0),
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

pub(crate) fn sys_mlock(args : SyscallArgs) -> UserRet {
    let addr = args.arg(0);
    let len = args.arg(1);
    if validate_mlock_range(addr, len).is_err() {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_munlock(args : SyscallArgs) -> UserRet {
    let addr = args.arg(0);
    let len = args.arg(1);
    if validate_mlock_range(addr, len).is_err() {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    UserRet::from_success(0)
}

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

pub(crate) fn sys_munlockall(_args : SyscallArgs) -> UserRet { UserRet::from_success(0) }
