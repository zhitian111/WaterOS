//! `mmap` / `munmap` / `mprotect`：经 `user_aspace` 句柄拼合 `MmapOps`。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::mm_util::{
    current_user_aspace_handle, linux_mmap_flags_to_map_flags, linux_mmap_prot_to_perm,
    mm_err_to_errno,
};

pub(crate) fn sys_mmap(args : SyscallArgs) -> UserRet {
    let Some(handle) = current_user_aspace_handle() else {
        return UserRet::from_error(ErrNo::ENOSYS);
    };
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::{MmapKind, MmapOps, MmapRequest};
    use mm::frame_alloctor::GlobalPhysFrameAllocator;
    let addr = args.arg(0);
    let len = args.arg(1);
    let prot = args.arg(2) as i32;
    let flags = args.arg(3) as u32;
    let _fd = args.arg(4);
    let _offset = args.arg(5);
    let perm = linux_mmap_prot_to_perm(prot);
    let mf = linux_mmap_flags_to_map_flags(flags);
    let addr_hint = if addr != 0 {
        Some(VirtAddr(addr))
    } else {
        None
    };
    let req = MmapRequest { addr_hint,
                            len,
                            prot : perm,
                            flags : mf,
                            kind : MmapKind::Anonymous };
    match mm::user_aspace::with_user_aspace_mut(handle, |aspace| {
              let mut alloc = GlobalPhysFrameAllocator;
              let base = MmapOps::mmap(aspace, &mut alloc, req)?;
              Ok(base.0)
          }) {
        Ok(base) => UserRet::from_success(base),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

pub(crate) fn sys_munmap(args : SyscallArgs) -> UserRet {
    let Some(handle) = current_user_aspace_handle() else {
        return UserRet::from_error(ErrNo::ENOSYS);
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

pub(crate) fn sys_mprotect(args : SyscallArgs) -> UserRet {
    let Some(handle) = current_user_aspace_handle() else {
        return UserRet::from_error(ErrNo::ENOSYS);
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
