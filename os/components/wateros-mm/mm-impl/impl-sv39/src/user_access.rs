//! 本模块代码由AI完成
//! Sv39 用户缓冲区 [`api_v0::user_access::UserMemoryOps`]。
//!
//! - **读/写**（`copy_from_user` / `copy_to_user`）：软件 walk 用户页表 + 内核恒等访问 PA，
//!   避免在 syscall 处理中临时切换 `satp`（ioctl 成功路径依赖 caller-saved 寄存器保真）。

use api_v0::addr::{PhysAddr, VirtAddr, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::mmap::PageFaultAccess;
use api_v0::perm::PagePerm;
use api_v0::user_access::UserMemoryOps;
use frame_alloctor::GlobalPhysFrameAllocator;

use crate::pagetable::Sv39AddressSpace;
use crate::user_aspace;

/// 绑定到指定用户地址空间句柄的拷贝实现。
pub struct Sv39UserMemoryOps {
    handle: usize,
}

impl Sv39UserMemoryOps {
    #[inline]
    pub const fn new(handle: usize) -> Self {
        Self { handle }
    }
}

impl UserMemoryOps for Sv39UserMemoryOps {
    fn copy_from_user(&self, dst: &mut [u8], src: VirtAddr) -> MmResult<usize> {
        user_copy(self.handle, dst, src)
    }

    fn copy_to_user(&self, dst: VirtAddr, src: &[u8]) -> MmResult<usize> {
        user_copy_to(self.handle, src, dst)
    }
}

/// 诊断：翻译用户 VA 并返回 satp（供 readlinkat 等失败路径 trace）。
pub fn debug_probe_user_virt(handle: usize, va: VirtAddr) -> MmResult<UserVirtProbe> {
    user_aspace::with_user_aspace_mut(handle, |aspace| {
        let pa = aspace.translate_addr(va)?;
        let perm = aspace.leaf_page_perm(va.floor_page())?;
        Ok(UserVirtProbe {
            pa,
            perm,
            aspace_satp: aspace.satp_value(),
        })
    })
}

/// [`debug_probe_user_virt`] 结果。
#[derive(Clone, Copy, Debug)]
pub struct UserVirtProbe {
    pub pa: Option<PhysAddr>,
    pub perm: Option<PagePerm>,
    pub aspace_satp: usize,
}

fn user_copy(
    handle: usize,
    kernel_buf: &mut [u8],
    user_addr: VirtAddr,
) -> MmResult<usize> {
    if kernel_buf.is_empty() {
        return Ok(0);
    }
    user_aspace::with_user_aspace_mut(handle, |aspace| {
        copy_from_user_in_aspace(aspace, kernel_buf, user_addr)
    })
}

fn user_copy_to(handle: usize, kernel_src: &[u8], user_addr: VirtAddr) -> MmResult<usize> {
    if kernel_src.is_empty() {
        return Ok(0);
    }
    user_aspace::with_user_aspace_mut(handle, |aspace| {
        copy_to_user_in_aspace(aspace, user_addr, kernel_src)
    })
}

fn copy_from_user_in_aspace(
    aspace: &mut Sv39AddressSpace,
    kernel_buf: &mut [u8],
    mut user_addr: VirtAddr,
) -> MmResult<usize> {
    let mut done = 0usize;
    while done < kernel_buf.len() {
        let pa = match aspace.translate_addr(user_addr)? {
            Some(pa) => pa,
            None => {
                let mut allocator = GlobalPhysFrameAllocator;
                if aspace.handle_lazy_page_fault(&mut allocator, user_addr, PageFaultAccess::Read)? {
                    match aspace.translate_addr(user_addr)? {
                        Some(pa) => pa,
                        None => return Err(MmError::AccessViolation),
                    }
                } else {
                    return Err(MmError::AccessViolation);
                }
            }
        };
        let perm = aspace
            .leaf_page_perm(user_addr.floor_page())?
            .ok_or(MmError::AccessViolation)?;
        if !perm.user() || !perm.readable() {
            return Err(MmError::AccessViolation);
        }

        let page_room = PAGE_SIZE - user_addr.page_offset();
        let chunk = page_room.min(kernel_buf.len() - done);
        let src = unsafe { core::slice::from_raw_parts(pa.0 as *const u8, chunk) };
        kernel_buf[done..done + chunk].copy_from_slice(src);
        done += chunk;
        user_addr = VirtAddr(user_addr.0.checked_add(chunk).ok_or(MmError::AccessViolation)?);
    }
    Ok(done)
}

fn copy_to_user_in_aspace(
    aspace: &mut Sv39AddressSpace,
    mut user_addr: VirtAddr,
    kernel_src: &[u8],
) -> MmResult<usize> {
    let mut done = 0usize;
    while done < kernel_src.len() {
        let vpn = user_addr.floor_page();
        let mut perm = match aspace.leaf_page_perm(vpn)? {
            Some(perm) => perm,
            None => {
                let mut allocator = GlobalPhysFrameAllocator;
                if aspace.handle_lazy_page_fault(&mut allocator, user_addr, PageFaultAccess::Write)? {
                    aspace
                        .leaf_page_perm(vpn)?
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
            perm = aspace
                .leaf_page_perm(vpn)?
                .ok_or(MmError::AccessViolation)?;
        }
        if !perm.writable() {
            return Err(MmError::AccessViolation);
        }
        let pa = match aspace.translate_addr(user_addr)? {
            Some(pa) => pa,
            None => return Err(MmError::AccessViolation),
        };

        let page_room = PAGE_SIZE - user_addr.page_offset();
        let chunk = page_room.min(kernel_src.len() - done);
        let dst = unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut u8, chunk) };
        dst.copy_from_slice(&kernel_src[done..done + chunk]);
        done += chunk;
        user_addr = VirtAddr(user_addr.0.checked_add(chunk).ok_or(MmError::AccessViolation)?);
    }
    Ok(done)
}
