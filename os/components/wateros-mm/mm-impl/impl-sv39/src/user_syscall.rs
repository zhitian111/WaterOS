//! 供 `wateros-syscall` 等路径在用户页表上执行 `brk`/`mmap`/`munmap`/`mprotect` 的薄封装。
//! 调用方须保证 `aspace_ptr` 指向 [`crate::pagetable::Sv39AddressSpace`] 且与当前任务的 `satp` 一致。

use api_v0::addr::VirtAddr;
use api_v0::address_space::AddressSpaceOps;
use api_v0::brk::HeapBrk;
use api_v0::error::MmResult;
use api_v0::flags::MapFlags;
use api_v0::mmap::{MmapKind, MmapOps, MmapRequest};
use api_v0::perm::PagePerm;

use frame_alloctor::GlobalPhysFrameAllocator;

use crate::pagetable::Sv39AddressSpace;

#[inline]
unsafe fn aspace_mut(aspace_ptr: usize) -> Option<&'static mut Sv39AddressSpace> {
    if aspace_ptr == 0 {
        return None;
    }
    Some(unsafe { &mut *(aspace_ptr as *mut Sv39AddressSpace) })
}

/// `brk(0)` 或 `brk(addr)`；`addr==0` 时返回当前 program break。
pub fn brk(aspace_ptr: usize, addr: usize) -> MmResult<usize> {
    let a = unsafe { aspace_mut(aspace_ptr).ok_or(api_v0::error::MmError::InvalidAddress)? };
    let mut alloc = GlobalPhysFrameAllocator;
    if addr == 0 {
        return Ok(HeapBrk::brk_region(a).current_end.0);
    }
    let new = HeapBrk::brk(a, &mut alloc, VirtAddr(addr))?;
    Ok(new.0)
}

/// 匿名私有 `mmap` 最小子集。
pub fn mmap(
    aspace_ptr: usize,
    addr: usize,
    len: usize,
    prot: PagePerm,
    flags: u32,
    _fd: usize,
    _offset: usize,
) -> MmResult<usize> {
    let a = unsafe { aspace_mut(aspace_ptr).ok_or(api_v0::error::MmError::InvalidAddress)? };
    let mut alloc = GlobalPhysFrameAllocator;
    const MAP_ANONYMOUS: u32 = 0x20;
    const MAP_PRIVATE: u32 = 0x02;
    const MAP_FIXED: u32 = 0x10;
    let mut mf = MapFlags::empty();
    if flags & MAP_ANONYMOUS != 0 {
        mf |= MapFlags::ANONYMOUS;
    }
    if flags & MAP_PRIVATE != 0 {
        mf |= MapFlags::PRIVATE;
    }
    if flags & MAP_FIXED != 0 {
        mf |= MapFlags::FIXED;
    }
    let addr_hint = if addr != 0 { Some(VirtAddr(addr)) } else { None };
    let req = MmapRequest {
        addr_hint,
        len,
        prot,
        flags: mf,
        kind: MmapKind::Anonymous,
    };
    let base = MmapOps::mmap(a, &mut alloc, req)?;
    Ok(base.0)
}

/// `munmap`。
pub fn munmap(aspace_ptr: usize, addr: usize, len: usize) -> MmResult<()> {
    let a = unsafe { aspace_mut(aspace_ptr).ok_or(api_v0::error::MmError::InvalidAddress)? };
    let mut alloc = GlobalPhysFrameAllocator;
    MmapOps::munmap(a, &mut alloc, VirtAddr(addr), len)
}

/// `mprotect`；`len==0` 时由实现层直接成功返回。
pub fn mprotect(aspace_ptr: usize, addr: usize, len: usize, perm: PagePerm) -> MmResult<()> {
    let a = unsafe { aspace_mut(aspace_ptr).ok_or(api_v0::error::MmError::InvalidAddress)? };
    MmapOps::mprotect(a, VirtAddr(addr), len, perm)
}

/// bring-up：内核态用翻译后的物理地址写用户映射页（不经过用户指针）。
pub fn probe_write_user_page(aspace_ptr: usize, user_va: usize, byte: u8) -> MmResult<()> {
    let a = unsafe { aspace_mut(aspace_ptr).ok_or(api_v0::error::MmError::InvalidAddress)? };
    let va = VirtAddr(user_va);
    let pa = a
        .translate_addr(va)?
        .ok_or(api_v0::error::MmError::NotMapped)?;
    unsafe {
        (pa.0 as *mut u8).write_volatile(byte);
    }
    Ok(())
}

/// bring-up：unmap 后翻译应为 `None`。
pub fn translate_user(aspace_ptr: usize, user_va: usize) -> MmResult<Option<usize>> {
    let a = unsafe { aspace_mut(aspace_ptr).ok_or(api_v0::error::MmError::InvalidAddress)? };
    Ok(a.translate_addr(VirtAddr(user_va))?.map(|p| p.0))
}
