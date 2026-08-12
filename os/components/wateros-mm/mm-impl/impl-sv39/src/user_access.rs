//! 本模块代码由AI完成
//! Sv39 用户缓冲区 [`api_v0::user_access::UserMemoryOps`]。
//!
//! - **读/写**（`copy_from_user` / `copy_to_user`）：软件 walk 用户页表 + 内核恒等访问 PA，
//!   避免在 syscall 处理中临时切换 `satp`（ioctl 成功路径依赖 caller-saved 寄存器保真）。

use api_v0::addr::{PhysAddr, VirtAddr, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::mmap::{MmapOps, PageFaultAccess};
use api_v0::perm::PagePerm;
use api_v0::user_access::{FutexMappingIdentity, UserCopyProgress, UserCopySource, UserMemoryOps};
use core::sync::atomic::{AtomicU32, Ordering};
use frame_alloctor::GlobalPhysFrameAllocator;

use crate::pagetable::Sv39AddressSpace;
use crate::user_aspace;

/// 绑定到指定用户地址空间句柄的拷贝实现。
pub struct Sv39UserMemoryOps {
    handle : usize,
}

impl Sv39UserMemoryOps {
    pub const fn new(handle : usize) -> Self { Self { handle } }
}

impl UserMemoryOps for Sv39UserMemoryOps {
    fn copy_from_user(&self, dst : &mut [u8], src : VirtAddr) -> MmResult<usize> {
        user_copy(self.handle, dst, src)
    }

    fn copy_to_user_progress(&self, dst : VirtAddr, src : &[u8]) -> UserCopyProgress {
        user_copy_to_progress(self.handle, src, dst)
    }

    fn copy_source_to_user_progress(&self,
                                    dst : VirtAddr,
                                    source : &dyn UserCopySource)
                                    -> UserCopyProgress {
        user_copy_source_to_progress(self.handle, source, dst)
    }

    fn atomic_load_u32(&self, src : VirtAddr) -> MmResult<u32> {
        atomic_load_user_u32(self.handle, src)
    }

    fn atomic_compare_exchange_u32(&self,
                                   dst : VirtAddr,
                                   expected : u32,
                                   desired : u32)
                                   -> MmResult<u32> {
        atomic_compare_exchange_user_u32(self.handle, dst, expected, desired)
    }

    fn futex_mapping_identity_u32(&self,
                                  src : VirtAddr)
                                  -> MmResult<FutexMappingIdentity> {
        futex_mapping_identity_user_u32(self.handle, src)
    }
}

/// 诊断：翻译用户 VA 并返回 satp（供 readlinkat 等失败路径 trace）。
pub fn debug_probe_user_virt(handle : usize, va : VirtAddr) -> MmResult<UserVirtProbe> {
    user_aspace::with_user_aspace_mut(handle, |aspace| {
        let pa = aspace.translate_addr(va)?;
        let perm = aspace.leaf_page_perm(va.floor_page())?;
        Ok(UserVirtProbe { pa,
                           perm,
                           aspace_satp : aspace.satp_value() })
    })
}

/// [`debug_probe_user_virt`] 结果。
#[derive(Clone, Copy, Debug)]
pub struct UserVirtProbe {
    pub pa : Option<PhysAddr>,
    pub perm : Option<PagePerm>,
    pub aspace_satp : usize,
}

fn user_copy(handle : usize, kernel_buf : &mut [u8], user_addr : VirtAddr) -> MmResult<usize> {
    if kernel_buf.is_empty() {
        return Ok(0);
    }
    user_aspace::with_user_aspace_mut(handle, |aspace| {
        copy_from_user_in_aspace(aspace, kernel_buf, user_addr)
    })
}

fn user_copy_to_progress(handle : usize,
                         kernel_src : &[u8],
                         user_addr : VirtAddr)
                         -> UserCopyProgress {
    if kernel_src.is_empty() {
        return UserCopyProgress::complete(0);
    }
    match user_aspace::with_user_aspace_mut(handle, |aspace| {
              Ok(copy_to_user_in_aspace(aspace,
                                        user_addr,
                                        kernel_src))
          }) {
        Ok(progress) => progress,
        Err(error) => UserCopyProgress::failed(0, error),
    }
}

fn user_copy_source_to_progress(handle : usize,
                                source : &dyn UserCopySource,
                                mut user_addr : VirtAddr)
                                -> UserCopyProgress {
    match user_aspace::with_user_aspace_mut(handle, |aspace| {
              let mut copied = 0usize;
              let mut error = None;
              source.visit(&mut |bytes| {
                  let progress = copy_to_user_in_aspace(aspace, user_addr, bytes);
                  copied += progress.copied;
                  error = progress.error;
                  if error.is_none() && progress.copied == bytes.len() {
                      if let Some(next) = user_addr.0.checked_add(bytes.len()) {
                          user_addr = VirtAddr(next);
                          true
                      } else {
                          error = Some(MmError::InvalidAddress);
                          false
                      }
                  } else {
                      false
                  }
              });
              Ok(match error {
                  Some(error) => UserCopyProgress::failed(copied, error),
                  None => UserCopyProgress::complete(copied),
              })
          }) {
        Ok(progress) => progress,
        Err(error) => UserCopyProgress::failed(0, error),
    }
}

fn validate_atomic_u32_addr(user_addr : VirtAddr) -> MmResult<()> {
    if user_addr.0 % core::mem::align_of::<u32>() != 0 ||
       user_addr.page_offset() > PAGE_SIZE - core::mem::size_of::<u32>()
    {
        return Err(MmError::InvalidAddress);
    }
    Ok(())
}

fn atomic_load_user_u32(handle : usize, user_addr : VirtAddr) -> MmResult<u32> {
    validate_atomic_u32_addr(user_addr)?;
    user_aspace::with_user_aspace_mut(handle, |aspace| {
        let pa = match aspace.translate_addr(user_addr)? {
            Some(pa) => pa,
            None => {
                let mut allocator = GlobalPhysFrameAllocator;
                if !MmapOps::handle_page_fault(aspace,
                                               &mut allocator,
                                               user_addr,
                                               PageFaultAccess::Read)?
                {
                    return Err(MmError::AccessViolation);
                }
                aspace.translate_addr(user_addr)?
                      .ok_or(MmError::AccessViolation)?
            }
        };
        let perm = aspace.leaf_page_perm(user_addr.floor_page())?
                         .ok_or(MmError::AccessViolation)?;
        if !perm.user() || !perm.readable() {
            return Err(MmError::AccessViolation);
        }
        let value = unsafe { &*(pa.0 as *const AtomicU32) }.load(Ordering::SeqCst);
        Ok(value)
    })
}

fn futex_mapping_identity_user_u32(handle : usize,
                                   user_addr : VirtAddr)
                                   -> MmResult<FutexMappingIdentity> {
    validate_atomic_u32_addr(user_addr)?;
    user_aspace::with_user_aspace_mut(handle, |aspace| {
        let pa = match aspace.translate_addr(user_addr)? {
            Some(pa) => pa,
            None => {
                let mut allocator = GlobalPhysFrameAllocator;
                if !MmapOps::handle_page_fault(aspace,
                                               &mut allocator,
                                               user_addr,
                                               PageFaultAccess::Read)?
                {
                    return Err(MmError::AccessViolation);
                }
                aspace.translate_addr(user_addr)?
                      .ok_or(MmError::AccessViolation)?
            }
        };
        let perm = aspace.leaf_page_perm(user_addr.floor_page())?
                         .ok_or(MmError::AccessViolation)?;
        if !perm.user() || !perm.readable() {
            return Err(MmError::AccessViolation);
        }
        if aspace.shared_vma_contains(user_addr.floor_page()
                                               .start_addr()) {
            Ok(FutexMappingIdentity::Shared(pa.0))
        } else {
            Ok(FutexMappingIdentity::Private)
        }
    })
}

fn atomic_compare_exchange_user_u32(handle : usize,
                                    user_addr : VirtAddr,
                                    expected : u32,
                                    desired : u32)
                                    -> MmResult<u32> {
    validate_atomic_u32_addr(user_addr)?;
    user_aspace::with_user_aspace_mut(handle, |aspace| {
        let vpn = user_addr.floor_page();
        let mut perm = match aspace.leaf_page_perm(vpn)? {
            Some(perm) => perm,
            None => {
                let mut allocator = GlobalPhysFrameAllocator;
                if !MmapOps::handle_page_fault(aspace,
                                               &mut allocator,
                                               user_addr,
                                               PageFaultAccess::Write)?
                {
                    return Err(MmError::AccessViolation);
                }
                aspace.leaf_page_perm(vpn)?
                      .ok_or(MmError::AccessViolation)?
            }
        };
        if !perm.user() {
            return Err(MmError::AccessViolation);
        }
        if perm.writable() {
            if !aspace.ensure_private_for_write(vpn)? {
                return Err(MmError::AccessViolation);
            }
        } else if aspace.handle_cow_fault(user_addr)? {
            perm = aspace.leaf_page_perm(vpn)?
                         .ok_or(MmError::AccessViolation)?;
        }
        if !perm.writable() {
            return Err(MmError::AccessViolation);
        }
        let pa = aspace.translate_addr(user_addr)?
                       .ok_or(MmError::AccessViolation)?;
        let atomic = unsafe { &*(pa.0 as *const AtomicU32) };
        Ok(match atomic.compare_exchange(expected,
                                         desired,
                                         Ordering::SeqCst,
                                         Ordering::SeqCst)
           {
               Ok(old) | Err(old) => old,
           })
    })
}

fn copy_from_user_in_aspace(aspace : &mut Sv39AddressSpace,
                            kernel_buf : &mut [u8],
                            mut user_addr : VirtAddr)
                            -> MmResult<usize> {
    let mut done = 0usize;
    while done < kernel_buf.len() {
        let (pa, perm) = match aspace.translate_addr_with_perm(user_addr)? {
            Some(entry) => entry,
            None => {
                let mut allocator = GlobalPhysFrameAllocator;
                if MmapOps::handle_page_fault(aspace,
                                              &mut allocator,
                                              user_addr,
                                              PageFaultAccess::Read)?
                {
                    match aspace.translate_addr_with_perm(user_addr)? {
                        Some(entry) => entry,
                        None => return Err(MmError::AccessViolation),
                    }
                } else {
                    return Err(MmError::AccessViolation);
                }
            }
        };
        if !perm.user() || !perm.readable() {
            return Err(MmError::AccessViolation);
        }

        let page_room = PAGE_SIZE - user_addr.page_offset();
        let chunk = page_room.min(kernel_buf.len() - done);
        let src = unsafe { core::slice::from_raw_parts(pa.0 as *const u8, chunk) };
        kernel_buf[done..done + chunk].copy_from_slice(src);
        done += chunk;
        user_addr = VirtAddr(user_addr.0
                                      .checked_add(chunk)
                                      .ok_or(MmError::AccessViolation)?);
    }
    Ok(done)
}

fn copy_to_user_in_aspace(aspace : &mut Sv39AddressSpace,
                          mut user_addr : VirtAddr,
                          kernel_src : &[u8])
                          -> UserCopyProgress {
    if kernel_src.is_empty() {
        return UserCopyProgress::complete(0);
    }
    if user_addr.0
                .checked_add(kernel_src.len() - 1)
                .is_none()
    {
        return UserCopyProgress::failed(0, MmError::InvalidAddress);
    }

    let mut done = 0usize;
    while done < kernel_src.len() {
        let step = (|| -> MmResult<usize> {
            let vpn = user_addr.floor_page();
            let mut perm = match aspace.leaf_page_perm(vpn)? {
                Some(perm) => perm,
                None => {
                    let mut allocator = GlobalPhysFrameAllocator;
                    if MmapOps::handle_page_fault(aspace,
                                                  &mut allocator,
                                                  user_addr,
                                                  PageFaultAccess::Write)?
                    {
                        aspace.leaf_page_perm(vpn)?
                              .ok_or(MmError::AccessViolation)?
                    } else {
                        return Err(MmError::AccessViolation);
                    }
                }
            };
            if !perm.user() {
                return Err(MmError::AccessViolation);
            }
            if perm.writable() {
                if !aspace.ensure_private_for_write(vpn)? {
                    return Err(MmError::AccessViolation);
                }
            } else if aspace.handle_cow_fault(user_addr)? {
                perm = aspace.leaf_page_perm(vpn)?
                             .ok_or(MmError::AccessViolation)?;
            }
            if !perm.writable() {
                return Err(MmError::AccessViolation);
            }
            let pa = aspace.translate_addr(user_addr)?
                           .ok_or(MmError::AccessViolation)?;

            let page_room = PAGE_SIZE - user_addr.page_offset();
            let chunk = page_room.min(kernel_src.len() - done);
            let dst = unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut u8, chunk) };
            dst.copy_from_slice(&kernel_src[done..done + chunk]);
            Ok(chunk)
        })();
        match step {
            Ok(chunk) => {
                done += chunk;
                if done < kernel_src.len() {
                    let Some(next) = user_addr.0
                                              .checked_add(chunk)
                    else {
                        return UserCopyProgress::failed(done, MmError::InvalidAddress);
                    };
                    user_addr = VirtAddr(next);
                }
            }
            Err(error) => return UserCopyProgress::failed(done, error),
        }
    }
    UserCopyProgress::complete(done)
}

pub fn test_copy_to_user_progress() {
    use api_v0::addr::VirtPageNum;
    use frame_alloctor::{frame_alloc_result, frame_dealloc_result};

    let mut aspace = Sv39AddressSpace::new().expect("create user-copy test address space");
    let first = frame_alloc_result().expect("allocate first user-copy test page");
    let second = frame_alloc_result().expect("allocate second user-copy test page");
    let first_vpn = VirtPageNum(0x400);
    let second_vpn = VirtPageNum(first_vpn.0 + 1);
    let writable = PagePerm::R | PagePerm::W | PagePerm::U;
    aspace.map_page_to_ppn(first_vpn, first, writable)
          .expect("map first user-copy test page");
    aspace.map_page_to_ppn(second_vpn, second, writable)
          .expect("map second user-copy test page");

    let single = [0x31u8; 16];
    let single_va = VirtAddr(first_vpn.start_addr()
                                      .0 +
                             64);
    assert_eq!(copy_to_user_in_aspace(&mut aspace, single_va, &single),
               UserCopyProgress::complete(single.len()));
    let single_dst = unsafe {
        core::slice::from_raw_parts((first.0 * PAGE_SIZE + 64) as *const u8,
                                    single.len())
    };
    assert_eq!(single_dst, &single);

    let cross = [0x42u8; 16];
    let cross_va = VirtAddr(second_vpn.start_addr()
                                      .0 -
                            8);
    assert_eq!(copy_to_user_in_aspace(&mut aspace, cross_va, &cross),
               UserCopyProgress::complete(cross.len()));
    let first_tail = unsafe {
        core::slice::from_raw_parts((first.0 * PAGE_SIZE + PAGE_SIZE - 8) as *const u8,
                                    8)
    };
    let second_head =
        unsafe { core::slice::from_raw_parts((second.0 * PAGE_SIZE) as *const u8, 8) };
    assert_eq!(first_tail, &cross[..8]);
    assert_eq!(second_head, &cross[8..]);

    assert_eq!(aspace.unmap_page_to_ppn(second_vpn)
                     .expect("unmap second user-copy test page"),
               Some(second));
    frame_dealloc_result(second).expect("release second user-copy test page");
    let partial = copy_to_user_in_aspace(&mut aspace, cross_va, &cross);
    assert_eq!(partial,
               UserCopyProgress::failed(8, MmError::AccessViolation));

    aspace.protect_page(first_vpn, PagePerm::R | PagePerm::U)
          .expect("make user-copy test page read-only");
    assert_eq!(copy_to_user_in_aspace(&mut aspace, single_va, &single),
               UserCopyProgress::failed(0, MmError::AccessViolation));
    assert_eq!(copy_to_user_in_aspace(&mut aspace,
                                      VirtAddr(usize::MAX - 1),
                                      &[1; 4]),
               UserCopyProgress::failed(0, MmError::InvalidAddress));
    assert_eq!(copy_to_user_in_aspace(&mut aspace, VirtAddr(usize::MAX), &[]),
               UserCopyProgress::complete(0));
    drop(aspace);

    let mut parent = Sv39AddressSpace::new().expect("create COW parent address space");
    let cow_page = frame_alloc_result().expect("allocate COW user-copy test page");
    let cow_vpn = VirtPageNum(0x500);
    parent.map_page_to_ppn(cow_vpn, cow_page, writable)
          .expect("map COW user-copy test page");
    unsafe {
        *(cow_page.start_addr()
                  .0 as *mut u8) = 0x55;
    }
    let mut child = parent.fork_cow()
                          .expect("fork COW user-copy test address space");
    assert_eq!(copy_to_user_in_aspace(&mut child,
                                      cow_vpn.start_addr(),
                                      &[0xAA]),
               UserCopyProgress::complete(1));
    let parent_pa = parent.translate_addr(cow_vpn.start_addr())
                          .expect("translate COW parent")
                          .expect("COW parent mapping");
    let child_pa = child.translate_addr(cow_vpn.start_addr())
                        .expect("translate COW child")
                        .expect("COW child mapping");
    assert_ne!(parent_pa.floor_page(),
               child_pa.floor_page());
    assert_eq!(unsafe { *(parent_pa.0 as *const u8) },
               0x55);
    assert_eq!(unsafe { *(child_pa.0 as *const u8) },
               0xAA);
}
