//! SysV 共享内存 syscall 子集（`shmget` / `shmctl` / `shmat` / `shmdt`）。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use ipc::shm::{ShmAttachInfo, ShmError, SHM_RDONLY};

use crate::mm_util::{current_user_aspace_handle, mm_err_to_errno};
use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const IPC_RMID : usize = 0;
const IPC_SET : usize = 1;
const IPC_STAT : usize = 2;
const IPC_INFO : usize = 3;
const IPC_64 : usize = 0x100;
const SHM_STAT : usize = 13;
const SHM_INFO : usize = 14;
const SHM_STAT_ANY : usize = 15;
const SHM_RND : usize = 0o20000;
const SHM_REMAP : usize = 0o40000;
const SHM_EXEC : usize = 0o100000;
const SHM_NORESERVE : usize = 0o10000;
const SHM_DEST : u32 = 0o1000;
const SHMMNI : usize = 4096;

/// Linux asm-generic `struct ipc64_perm`，RV64 与 LA64 使用相同布局。
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Ipc64Perm {
    key : i32,
    uid : u32,
    gid : u32,
    cuid : u32,
    cgid : u32,
    mode : u32,
    pad1 : u32,
    seq : u16,
    pad2 : u16,
    unused1 : u64,
    unused2 : u64,
}

/// Linux asm-generic `struct shmid64_ds`。
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Shmid64Ds {
    perm : Ipc64Perm,
    segsz : u64,
    atime : i64,
    dtime : i64,
    ctime : i64,
    cpid : i32,
    lpid : i32,
    nattch : u64,
    unused4 : u64,
    unused5 : u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Shminfo64 {
    shmmax : u64,
    shmmin : u64,
    shmmni : u64,
    shmseg : u64,
    shmall : u64,
    unused1 : u64,
    unused2 : u64,
    unused3 : u64,
    unused4 : u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ShmInfo {
    used_ids : i32,
    _padding : u32,
    shm_tot : u64,
    shm_rss : u64,
    shm_swp : u64,
    swap_attempts : u64,
    swap_successes : u64,
}

const _ : () = assert!(core::mem::size_of::<Ipc64Perm>() == 48);
const _ : () = assert!(core::mem::size_of::<Shmid64Ds>() == 112);
const _ : () = assert!(core::mem::size_of::<Shminfo64>() == 72);
const _ : () = assert!(core::mem::size_of::<ShmInfo>() == 48);

// 本方法代码由AI完成
pub(crate) fn sys_shmget(args : SyscallArgs) -> UserRet {
    let key = args.arg(0);
    let size = args.arg(1);
    let flags = args.arg(2);
    let allowed_flags = 0o777 | ipc::shm::IPC_CREAT | ipc::shm::IPC_EXCL | SHM_NORESERVE;
    if flags & !allowed_flags != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let credentials = cred::current_credentials();
    match ipc::shm::registry().lock()
                              .create_or_get_with_metadata(key,
                                                          size,
                                                          flags,
                                                          credentials.effective_uid.0,
                                                          credentials.effective_gid.0,
                                                          process_id(),
                                                          now_seconds())
    {
        Ok(shmid) => UserRet::from_success(shmid),
        Err(error) => UserRet::from_error(shm_error_to_errno(error)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_shmctl(args : SyscallArgs) -> UserRet {
    let shmid = args.arg(0);
    let cmd = args.arg(1) & !IPC_64;
    let pointer = args.arg(2);
    let credentials = cred::current_credentials();

    let result = match cmd {
        IPC_INFO => {
            if pointer == 0 {
                Err(ErrNo::EFAULT)
            } else {
                let stats = ipc::shm::registry().lock().stats();
                let page_size = mm::api::addr::PAGE_SIZE;
                let info = Shminfo64 {
                    shmmax : ipc::shm::MAX_SHM_SEGMENT_SIZE as u64,
                    shmmin : 1,
                    shmmni : SHMMNI as u64,
                    shmseg : SHMMNI as u64,
                    shmall : (ipc::shm::MAX_SHM_SEGMENT_SIZE / page_size * SHMMNI) as u64,
                    ..Shminfo64::default()
                };
                copy_to_user_struct(pointer, &info).map(|_| stats.max_id)
            }
        }
        SHM_INFO => {
            if pointer == 0 {
                Err(ErrNo::EFAULT)
            } else {
                let stats = ipc::shm::registry().lock().stats();
                let info = ShmInfo {
                    used_ids : stats.segment_count.min(i32::MAX as usize) as i32,
                    shm_tot : stats.total_pages as u64,
                    shm_rss : stats.total_pages as u64,
                    ..ShmInfo::default()
                };
                copy_to_user_struct(pointer, &info).map(|_| stats.max_id)
            }
        }
        IPC_STAT | SHM_STAT | SHM_STAT_ANY => {
            if pointer == 0 {
                Err(ErrNo::EFAULT)
            } else {
                let segment = ipc::shm::registry().lock()
                                                    .segment_info(shmid)
                                                    .map_err(shm_error_to_errno);
                segment.and_then(|segment| {
                    if cmd != SHM_STAT_ANY && !may_read_segment(&segment, &credentials) {
                        return Err(ErrNo::EACCES);
                    }
                    let snapshot = segment_snapshot(&segment);
                    copy_to_user_struct(pointer, &snapshot).map(|_| {
                        if cmd == IPC_STAT { 0 } else { segment.shmid }
                    })
                })
            }
        }
        IPC_SET => {
            if pointer == 0 {
                Err(ErrNo::EFAULT)
            } else {
                let update = copy_from_user_struct::<Shmid64Ds>(pointer);
                update.and_then(|update| {
                    let mut registry = ipc::shm::registry().lock();
                    let segment = registry.segment_info(shmid).map_err(shm_error_to_errno)?;
                    if !may_administer_segment(&segment, &credentials) {
                        return Err(ErrNo::EPERM);
                    }
                    registry.update_permissions(shmid,
                                                update.perm.uid,
                                                update.perm.gid,
                                                update.perm.mode as usize,
                                                now_seconds())
                            .map_err(shm_error_to_errno)?;
                    Ok(0)
                })
            }
        }
        IPC_RMID => {
            let mut registry = ipc::shm::registry().lock();
            let segment = registry.segment_info(shmid).map_err(shm_error_to_errno);
            segment.and_then(|segment| {
                if !may_administer_segment(&segment, &credentials) {
                    return Err(ErrNo::EPERM);
                }
                registry.mark_removed(shmid).map_err(shm_error_to_errno)?;
                Ok(0)
            })
        }
        _ => Err(ErrNo::EINVAL),
    };
    match result {
        Ok(value) => UserRet::from_success(value),
        Err(error) => UserRet::from_error(error),
    }
}

fn now_seconds() -> i64 {
    platform::wall_clock::realtime_ns()
        .map(|ns| (ns / 1_000_000_000).min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn process_id() -> i32 {
    task::current_process_snapshot()
        .map(|process| process.pid.raw().min(i32::MAX as usize) as i32)
        .unwrap_or(0)
}

fn may_read_segment(segment : &ipc::shm::ShmSegmentInfo,
                    credentials : &cred::api::ProcessCredentials)
                    -> bool {
    if credentials.effective_uid.0 == 0 {
        return true;
    }
    let shift = if credentials.effective_uid.0 == segment.owner_uid ||
                   credentials.effective_uid.0 == segment.creator_uid {
        6
    } else if credentials.effective_gid.0 == segment.owner_gid ||
              credentials.effective_gid.0 == segment.creator_gid ||
              credentials.supplementary_groups
                         .iter()
                         .take(credentials.supplementary_group_len)
                         .any(|gid| gid.0 == segment.owner_gid || gid.0 == segment.creator_gid)
    {
        3
    } else {
        0
    };
    ((segment.mode >> shift) & 0o4) != 0
}

fn may_administer_segment(segment : &ipc::shm::ShmSegmentInfo,
                          credentials : &cred::api::ProcessCredentials)
                          -> bool {
    credentials.effective_uid.0 == 0 ||
    credentials.effective_uid.0 == segment.owner_uid ||
    credentials.effective_uid.0 == segment.creator_uid
}

fn segment_snapshot(segment : &ipc::shm::ShmSegmentInfo) -> Shmid64Ds {
    Shmid64Ds {
        perm : Ipc64Perm {
            key : segment.key as i32,
            uid : segment.owner_uid,
            gid : segment.owner_gid,
            cuid : segment.creator_uid,
            cgid : segment.creator_gid,
            mode : segment.mode as u32 |
                   if segment.marked_removed { SHM_DEST } else { 0 },
            ..Ipc64Perm::default()
        },
        segsz : segment.size as u64,
        atime : segment.attach_time,
        dtime : segment.detach_time,
        ctime : segment.change_time,
        cpid : segment.creator_pid,
        lpid : segment.last_pid,
        nattch : segment.nattch as u64,
        ..Shmid64Ds::default()
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
    let allowed_flags = SHM_RDONLY | SHM_RND | SHM_REMAP | SHM_EXEC;
    if shmflg & !allowed_flags != 0 || (shmflg & SHM_REMAP != 0 && shmaddr == 0) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let readonly = shmflg & SHM_RDONLY != 0;
    let executable = shmflg & SHM_EXEC != 0;
    let credentials = cred::current_credentials();

    let (reservation, segment) = {
        let mut reg = ipc::shm::registry().lock();
        let segment = match reg.segment_info(shmid) {
            Ok(segment) => segment,
            Err(error) => return UserRet::from_error(shm_error_to_errno(error)),
        };
        if !may_attach_segment(&segment, &credentials, readonly, executable) {
            return UserRet::from_error(ErrNo::EACCES);
        }
        match reg.begin_attach(shmid) {
            Ok(reservation) => reservation,
            Err(error) => return UserRet::from_error(shm_error_to_errno(error)),
        }
    };
    let base = match reserve_attach_va(handle,
                                       shmaddr,
                                       shmflg,
                                       segment.size,
                                       readonly,
                                       executable)
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
                              .finish_attach_with_metadata(&reservation,
                                                           task_id(),
                                                           base,
                                                           readonly,
                                                           process_id(),
                                                           now_seconds())
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

fn may_attach_segment(segment : &ipc::shm::ShmSegmentInfo,
                      credentials : &cred::api::ProcessCredentials,
                      readonly : bool,
                      executable : bool)
                      -> bool {
    if credentials.effective_uid.0 == 0 {
        return true;
    }
    let permission = if credentials.effective_uid.0 == segment.owner_uid {
        (segment.mode >> 6) & 0o7
    } else if credentials.effective_gid.0 == segment.owner_gid ||
              credentials.supplementary_groups
                         .iter()
                         .take(credentials.supplementary_group_len)
                         .any(|gid| gid.0 == segment.owner_gid)
    {
        (segment.mode >> 3) & 0o7
    } else {
        segment.mode & 0o7
    };
    let mut required = if readonly { 0o4 } else { 0o6 };
    if executable {
        required |= 0o1;
    }
    permission & required == required
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
    match ipc::shm::registry().lock()
                              .detach_with_metadata(task_id(),
                                                    base,
                                                    process_id(),
                                                    now_seconds()) {
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
                     readonly : bool,
                     executable : bool)
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
        map_flags |= if flags & SHM_REMAP != 0 {
            MapFlags::FIXED
        } else {
            MapFlags::FIXED_NOREPLACE
        };
    }
    let mut prot = PagePerm::U | PagePerm::R;
    if !readonly {
        prot |= PagePerm::W;
    }
    if executable {
        prot |= PagePerm::X;
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
        let mut failed_deallocations = 0usize;
        for (index, ppn) in info.pages
                                .iter()
                                .copied()
                                .enumerate()
        {
            let vpn = VirtAddr(info.base + index * PAGE_SIZE).floor_page();
            if let Some(old_ppn) = aspace.unmap_page_to_ppn(vpn)? {
                if dealloc_old && old_ppn != ppn {
                    if frame_dealloc_result(old_ppn).is_err() {
                        failed_deallocations += 1;
                    }
                }
            }
            aspace.map_page_to_ppn(vpn, ppn, perm)?;
        }
        if failed_deallocations != 0 {
            log::warn!("[shm] replacement found already-free anonymous frames shmid={} failed={}",
                       info.shmid,
                       failed_deallocations);
        }
        Ok(())
    }).map_err(mm_err_to_errno)
}

fn unmap_shared_range(handle : usize, info : &ShmAttachInfo) -> Result<(), ErrNo> {
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::MmapOps;

    mm::user_aspace::with_user_aspace_mut_and_flush_if_changed(handle, |aspace| {
        let changed = MmapOps::munmap_external(aspace, VirtAddr(info.base), info.size)?;
        Ok(((), changed.changed()))
    }).map_err(mm_err_to_errno)
}

fn unmap_range_dealloc(handle : usize, base : usize, len : usize) -> Result<(), ErrNo> {
    use mm::api::addr::VirtAddr;
    use mm::api::mmap::MmapOps;
    use mm::frame_alloctor::GlobalPhysFrameAllocator;

    mm::user_aspace::with_user_aspace_mut_and_flush_if_changed(handle, |aspace| {
        let mut alloc = GlobalPhysFrameAllocator;
        let changed = MmapOps::munmap(aspace, &mut alloc, VirtAddr(base), len)?;
        Ok(((), changed.changed()))
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
