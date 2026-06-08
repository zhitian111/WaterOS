//! `mmap` / `munmap` / `mprotect`：经 `user_aspace` 句柄拼合 `MmapOps`。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::mm_util::{
    current_user_aspace_handle, linux_mmap_flags_to_map_flags, linux_mmap_is_anonymous,
    linux_mmap_prot_to_perm, mm_err_to_errno,
};
use crate::vfs_util::read_fd_bytes_at;
use api_v0::unsupported::syscall_unsupported;

pub(crate) fn sys_mmap(args: SyscallArgs) -> UserRet {
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

    let (kind, file_backing) = if linux_mmap_is_anonymous(flags) {
        if !mf.contains(MapFlags::PRIVATE) {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        (MmapKind::Anonymous, None)
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
        let backing = match read_fd_bytes_at(fd as usize, offset, len) {
            Ok(b) => b,
            Err(e) => return UserRet::from_error(e),
        };
        (
            MmapKind::File {
                fd: fd as usize,
                offset,
            },
            Some(backing),
        )
    };

    let req = MmapRequest {
        addr_hint,
        len,
        prot: perm,
        flags: mf,
        kind,
    };

    match file_backing {
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
        Some(backing) => {
            let backing: Vec<u8> = backing;
            match mm::user_aspace::with_user_aspace_mut(handle, |aspace| {
                let mut alloc = GlobalPhysFrameAllocator;
                let base = MmapOps::mmap(
                    aspace,
                    &mut alloc,
                    req,
                    Some(backing.as_slice()),
                )?;
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

pub(crate) fn sys_munmap(args: SyscallArgs) -> UserRet {
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

pub(crate) fn sys_msync(args: SyscallArgs) -> UserRet {
    use mm::api::addr::PAGE_SIZE;

    const MS_ASYNC: usize = 0x1;
    const MS_INVALIDATE: usize = 0x2;
    const MS_SYNC: usize = 0x4;
    const MS_KNOWN: usize = MS_ASYNC | MS_INVALIDATE | MS_SYNC;

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

pub(crate) fn sys_mprotect(args: SyscallArgs) -> UserRet {
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
