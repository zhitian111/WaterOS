//! Sv39 用户缓冲区 [`api_v0::user_access::UserMemoryOps`]。
//!
//! - **读**（`copy_from_user`）：软件 walk + 内核恒等写 PA（与 bring-up 一致）。
//! - **写**（`copy_to_user`）：临时激活用户 `satp` + `SUM` 直访用户 VA，
//!   避免恒等写路径在栈等页上误判；切换期间关中断以防嵌套 trap 时 satp 错乱。

use api_v0::addr::{PhysAddr, VirtAddr, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::kernel_satp;
use api_v0::perm::PagePerm;
use api_v0::user_access::UserMemoryOps;

use crate::pagetable::Sv39AddressSpace;
use crate::user_aspace;

/// 绑定到指定用户地址空间句柄的拷贝实现。
pub struct Sv39UserMemoryOps {
    handle: usize,
}

impl Sv39UserMemoryOps {
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

fn user_satp_for_handle(handle: usize) -> MmResult<usize> {
    user_aspace::with_user_aspace_mut(handle, |aspace| Ok(aspace.satp_value()))
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
    let user_satp = user_satp_for_handle(handle)?;
    copy_to_user_via_satp(user_satp, user_addr, kernel_src)
}

fn copy_from_user_in_aspace(
    aspace: &Sv39AddressSpace,
    kernel_buf: &mut [u8],
    mut user_addr: VirtAddr,
) -> MmResult<usize> {
    let mut done = 0usize;
    while done < kernel_buf.len() {
        let pa = match aspace.translate_addr(user_addr)? {
            Some(pa) => pa,
            None => return Err(MmError::AccessViolation),
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

fn copy_to_user_via_satp(
    user_satp: usize,
    mut user_addr: VirtAddr,
    kernel_src: &[u8],
) -> MmResult<usize> {
    if user_satp == 0 {
        return Err(MmError::InvalidAddress);
    }
    let kernel_satp = kernel_satp::get();
    let irq_state = platform::interrupt::read_global_interrupt_state().ok();
    let _ = platform::interrupt::disable_global_interrupt();
    platform::arch::paging::activate_address_space_token_and_flush(user_satp);
    platform::arch::trap::prepare_user_trap_frame_access();

    let result = copy_to_user_va(&mut user_addr, kernel_src);

    platform::arch::paging::activate_address_space_token_and_flush(kernel_satp);
    if let Some(state) = irq_state {
        let _ = platform::interrupt::restore_global_interrupt_state(state);
    }
    result
}

fn copy_to_user_va(user_addr: &mut VirtAddr, kernel_src: &[u8]) -> MmResult<usize> {
    let mut done = 0usize;
    while done < kernel_src.len() {
        let page_room = PAGE_SIZE - user_addr.page_offset();
        let chunk = page_room.min(kernel_src.len() - done);
        let user_ptr = user_addr.0 as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(
                kernel_src.as_ptr().add(done),
                user_ptr,
                chunk,
            );
        }
        done += chunk;
        *user_addr =
            VirtAddr(user_addr.0.checked_add(chunk).ok_or(MmError::AccessViolation)?);
    }
    Ok(done)
}
