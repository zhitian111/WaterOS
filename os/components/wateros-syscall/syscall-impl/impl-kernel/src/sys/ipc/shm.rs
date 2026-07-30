//! SysV 共享内存 syscall 子集（`shmget` / `shmctl` / `shmat` / `shmdt`）。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::shm::{ShmAttachInfo, ShmError, SHM_RDONLY};

use crate::mm_util::{current_user_aspace_handle, mm_err_to_errno};

const IPC_RMID : usize = 0;
const SHM_RND : usize = 0o20000;

// 本方法代码由AI完成
pub(crate) fn sys_shmget(args : SyscallArgs) -> UserRet {
    let key = args.arg(0);
    let size = args.arg(1);
    let flags = args.arg(2);
    match ipc::shm::registry().lock()
                              .create_or_get(key, size, flags)
    {
        Ok(shmid) => UserRet::from_success(shmid),
        Err(error) => UserRet::from_error(shm_error_to_errno(error)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_shmctl(args : SyscallArgs) -> UserRet {
    let shmid = args.arg(0);
    let cmd = args.arg(1);

    match cmd {
        IPC_RMID => match ipc::shm::registry().lock()
                                              .mark_removed(shmid)
        {
            Ok(()) => UserRet::from_success(0),
            Err(error) => UserRet::from_error(shm_error_to_errno(error)),
        },
        _ => UserRet::from_error(ErrNo::ENOSYS),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_shmat(args : SyscallArgs) -> UserRet {
    let Some(handle) = current_user_aspace_handle() else {
        return UserRet::from_error(ErrNo::EFAULT);
    };
    let shmid = args.arg(0);
    let shmaddr = args.arg(1);
    let shmflg = args.arg(2);
    let readonly = shmflg & SHM_RDONLY != 0;

    let (reservation, segment) = {
        let mut reg = ipc::shm::registry().lock();
        match reg.begin_attach(shmid) {
            Ok(reservation) => reservation,
            Err(error) => return UserRet::from_error(shm_error_to_errno(error)),
        }
    };
    let base = match reserve_attach_va(handle,
                                       shmaddr,
                                       shmflg,
                                       segment.size,
                                       readonly)
    {
        Ok(base) => base,
        Err(error) => {
            let _ = ipc::shm::registry().lock()
                                        .cancel_attach_reservation(&reservation);
            return UserRet::from_error(error);
        }
    };
    let info = ShmAttachInfo { shmid,
                               base,
                               size : segment.size,
                               readonly,
                               pages : segment.pages };
    if let Err(error) = replace_range_with_shared(handle, &info, true) {
        let _ = unmap_range_dealloc(handle, base, info.size);
        let _ = ipc::shm::registry().lock()
                                    .cancel_attach_reservation(&reservation);
        return UserRet::from_error(error);
    }

    match ipc::shm::registry().lock()
                              .finish_attach(&reservation, task_id(), base, readonly)
    {
        Ok(_) => UserRet::from_success(base),
        Err(error) => {
            let _ = unmap_shared_range(handle, &info);
            let _ = ipc::shm::registry().lock()
                                        .cancel_attach_reservation(&reservation);
            UserRet::from_error(shm_error_to_errno(error))
        }
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_shmdt(args : SyscallArgs) -> UserRet {
    let Some(handle) = current_user_aspace_handle() else {
        return UserRet::from_error(ErrNo::EFAULT);
    };
    let base = args.arg(0);
    // 先在锁内复制 attachment，再释放锁解除页表映射。attachment 仍保留在 registry 中，
    // 因而并发 IPC_RMID 不会在 unmap 前回收共享帧。
    let info = match ipc::shm::registry().lock()
                                         .attachment_info(task_id(), base)
    {
        Ok(info) => info,
        Err(error) => return UserRet::from_error(shm_error_to_errno(error)),
    };
    if let Err(error) = unmap_shared_range(handle, &info) {
        return UserRet::from_error(error);
    }
    match ipc::shm::registry().lock().detach(task_id(), base) {
        Ok(_) => UserRet::from_success(0),
        // 同一 task 不会同时在两个 CPU 上执行 syscall；此处失败意味着 registry 已被违规修改。
        Err(error) => UserRet::from_error(shm_error_to_errno(error)),
    }
}

pub(crate) fn drop_task_attachments(task_id : task::TaskId, aspace_handle : usize) {
    if aspace_handle == 0 {
        let _ = ipc::shm::registry().lock().drop_task(task_id);
        return;
    }
    let attached = ipc::shm::registry().lock().task_attachments(task_id);
    for info in &attached {
        if unmap_shared_range(aspace_handle, info).is_ok() {
            // 仅在地址空间不再引用帧后递减 nattch，避免把仍映射的页交还 allocator。
            let _ = ipc::shm::registry().lock().detach(task_id, info.base);
        }
    }
}

pub(crate) fn fork_task_attachments(parent : task::TaskId,
                                    child : task::TaskId,
                                    child_aspace_handle : usize)
                                    -> Result<(), ErrNo> {
    let attached = ipc::shm::registry().lock()
                                       .fork_task(parent, child);
    for info in &attached {
        if let Err(error) = replace_range_with_shared(child_aspace_handle, info, true) {
            // fork_task 已增加 child 的 nattch；先撤销所有可能已映射的共享页，再删除 child
            // attachment，确保 IPC_RMID 不能在页表仍引用这些帧时回收它们。
            for rollback in &attached {
                let _ = unmap_shared_range(child_aspace_handle, rollback);
            }
            let _ = ipc::shm::registry().lock().drop_task(child);
            return Err(error);
        }
    }
    Ok(())
}

fn reserve_attach_va(handle : usize,
                     shmaddr : usize,
                     flags : usize,
                     len : usize,
                     readonly : bool)
                     -> Result<usize, ErrNo> {
    use mm::api::addr::{VirtAddr, PAGE_SIZE};
    use mm::api::flags::MapFlags;
    use mm::api::mmap::{MmapKind, MmapOps, MmapRequest};
    use mm::api::perm::PagePerm;
    use mm::frame_alloctor::GlobalPhysFrameAllocator;

    let addr = if shmaddr == 0 {
        None
    } else if shmaddr % PAGE_SIZE == 0 {
        Some(shmaddr)
    } else if flags & SHM_RND != 0 {
        Some(shmaddr & !(PAGE_SIZE - 1))
    } else {
        return Err(ErrNo::EINVAL);
    };
    let mut map_flags = MapFlags::ANONYMOUS | MapFlags::SHARED;
    if addr.is_some() {
        map_flags |= MapFlags::FIXED;
    }
    let mut prot = PagePerm::U | PagePerm::R;
    if !readonly {
        prot |= PagePerm::W;
    }
    let req = MmapRequest { addr_hint : addr.map(VirtAddr),
                            len,
                            prot,
                            flags : map_flags,
                            kind : MmapKind::Anonymous };
    mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
        let mut alloc = GlobalPhysFrameAllocator;
        let base = MmapOps::mmap(aspace, &mut alloc, req, None)?;
        Ok(base.0)
    }).map_err(mm_err_to_errno)
}

fn replace_range_with_shared(handle : usize,
                             info : &ShmAttachInfo,
                             dealloc_old : bool)
                             -> Result<(), ErrNo> {
    use mm::api::addr::{VirtAddr, PAGE_SIZE};
    use mm::api::address_space::AddressSpaceOps;
    use mm::api::perm::PagePerm;
    use mm::frame_alloctor::frame_dealloc_result;

    let mut perm = PagePerm::U | PagePerm::R;
    if !info.readonly {
        perm |= PagePerm::W;
    }
    mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
        for (index, ppn) in info.pages
                                .iter()
                                .copied()
                                .enumerate()
        {
            let vpn = VirtAddr(info.base + index * PAGE_SIZE).floor_page();
            if let Some(old_ppn) = aspace.unmap_page_to_ppn(vpn)? {
                if dealloc_old && old_ppn != ppn {
                    let _ = frame_dealloc_result(old_ppn);
                }
            }
            aspace.map_page_to_ppn(vpn, ppn, perm)?;
        }
        Ok(())
    }).map_err(mm_err_to_errno)
}

fn unmap_shared_range(handle : usize, info : &ShmAttachInfo) -> Result<(), ErrNo> {
    use mm::api::addr::{VirtAddr, PAGE_SIZE};
    use mm::api::address_space::AddressSpaceOps;

    mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
        for index in 0..info.pages.len() {
            let vpn = VirtAddr(info.base + index * PAGE_SIZE).floor_page();
            let _ = aspace.unmap_page_to_ppn(vpn)?;
        }
        Ok(())
    }).map_err(mm_err_to_errno)
}

fn unmap_range_dealloc(handle : usize, base : usize, len : usize) -> Result<(), ErrNo> {
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::MmapOps;
    use mm::frame_alloctor::GlobalPhysFrameAllocator;

    mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
        let mut alloc = GlobalPhysFrameAllocator;
        MmapOps::munmap(aspace, &mut alloc, VirtAddr(base), len)
    }).map_err(mm_err_to_errno)
}

// 本方法代码由AI完成
fn shm_error_to_errno(error : ShmError) -> ErrNo {
    match error {
        ShmError::Invalid => ErrNo::EINVAL,
        ShmError::Exists => ErrNo::EEXIST,
        ShmError::NoEntry => ErrNo::ENOENT,
        ShmError::NoMem => ErrNo::ENOMEM,
        ShmError::NoSys => ErrNo::ENOSYS,
    }
}

fn task_id() -> task::TaskId {
    task::current_task_id().expect("shm syscall requires a current task")
}
