//! 用户堆 `brk` 与匿名/文件 `mmap`/`munmap`/`mprotect` 的 LoongArch64 实现。
//!
//! # 缺页与 trap
//!
//! 与 [`crate::pagetable`] 一致：映射 **饥渴（eager）** 建立；未映射用户 VA
//! 由硬件 page fault 进入 trap，本阶段不提供 demand paging。

use api_v0::addr::{PhysPageNum, VirtAddr, VirtPageNum};
use api_v0::address_space::AddressSpaceOps;
use api_v0::brk::{BrkRegion, HeapBrk};
use api_v0::error::{MmError, MmResult};
use api_v0::flags::MapFlags;
use api_v0::frame_allocator::PhysicalFrameAllocator;
use api_v0::mmap::{MmapKind, MmapOps, MmapRequest};
use api_v0::perm::PagePerm;
use impl_common::{map_range_from_backing, map_zeroed_page_with_alloc, map_zeroed_range_with_alloc, mmap_map_end};

use crate::pagetable::LoongArch64AddressSpace;

#[inline]
fn fence_user_ptes() { platform::arch::paging::flush_address_space_translations(); }

impl HeapBrk for LoongArch64AddressSpace {
    fn brk_region(&self) -> BrkRegion {
        BrkRegion { start : self.user_brk_start,
                    current_end : self.user_brk_current_end,
                    max : self.user_brk_max }
    }

    fn brk<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                              allocator : &mut A,
                                                              new_end : VirtAddr)
                                                              -> MmResult<VirtAddr> {
        if new_end.0 == 0 {
            return Ok(self.user_brk_current_end);
        }
        let r = self.brk_region();
        if new_end.0 < r.start.0 {
            return Err(MmError::InvalidAddress);
        }
        if new_end.0 < r.current_end.0 {
            return Err(MmError::InvalidAddress);
        }
        if new_end.0 > r.max.0 {
            return Err(MmError::InvalidAddress);
        }
        if new_end.0 > r.current_end.0 {
            let end_vpn_excl = VirtAddr(new_end.0).ceil_page()
                                                  .0;
            let mut vpn_i = VirtAddr(r.current_end.0).floor_page()
                                                     .0;
            while vpn_i < end_vpn_excl {
                let vpn = VirtPageNum(vpn_i);
                if self.translate_addr(vpn.start_addr())?
                       .is_none()
                {
                    map_zeroed_page_with_alloc(self, allocator, vpn, Self::brk_perm())?;
                }
                vpn_i += 1;
            }
        }
        self.user_brk_current_end = new_end;
        fence_user_ptes();
        Ok(new_end)
    }
}

impl LoongArch64AddressSpace {
    fn mmap_anonymous<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                         allocator : &mut A,
                                                                         req : MmapRequest)
                                                                         -> MmResult<VirtAddr> {
        if !req.flags
               .contains(MapFlags::ANONYMOUS) ||
           !req.flags
               .contains(MapFlags::PRIVATE)
        {
            return Err(MmError::InvalidAddress);
        }
        let base = match req.addr_hint {
            Some(hint)
                if req.flags
                      .contains(MapFlags::FIXED) =>
            {
                hint
            }
            Some(_) => return Err(MmError::InvalidAddress),
            None => self.mmap_anon_cursor,
        };
        let end = mmap_map_end(base, req.len)?;
        let perm = req.prot | PagePerm::U;
        if perm == PagePerm::U {
            return Err(MmError::InvalidAddress);
        }
        map_zeroed_range_with_alloc(self, allocator, base, end, perm)?;
        if req.addr_hint
              .is_none()
        {
            self.mmap_anon_cursor = end;
        }
        Ok(base)
    }

    fn mmap_file<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                    allocator : &mut A,
                                                                    req : MmapRequest,
                                                                    file_backing : &[u8])
                                                                    -> MmResult<VirtAddr> {
        if req.flags
              .contains(MapFlags::ANONYMOUS)
        {
            return Err(MmError::InvalidAddress);
        }
        if !req.flags
               .contains(MapFlags::SHARED) &&
           !req.flags
               .contains(MapFlags::PRIVATE)
        {
            return Err(MmError::InvalidAddress);
        }
        if file_backing.len() != req.len {
            return Err(MmError::InvalidAddress);
        }
        let base = match req.addr_hint {
            Some(hint)
                if req.flags
                      .contains(MapFlags::FIXED) =>
            {
                hint
            }
            Some(_) => return Err(MmError::InvalidAddress),
            None => self.mmap_file_cursor,
        };
        let end = mmap_map_end(base, req.len)?;
        let perm = req.prot | PagePerm::U;
        if perm == PagePerm::U {
            return Err(MmError::InvalidAddress);
        }
        map_range_from_backing(self, allocator, base, end, perm, file_backing)?;
        if req.addr_hint
              .is_none()
        {
            self.mmap_file_cursor = end;
        }
        Ok(base)
    }
}

impl MmapOps for LoongArch64AddressSpace {
    fn mmap<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                               allocator : &mut A,
                                                               req : MmapRequest,
                                                               file_backing : Option<&[u8]>)
                                                               -> MmResult<VirtAddr> {
        if req.len == 0 {
            return Err(MmError::InvalidAddress);
        }
        match req.kind {
            MmapKind::Anonymous => {
                if file_backing.is_some() {
                    return Err(MmError::InvalidAddress);
                }
                self.mmap_anonymous(allocator, req)
            }
            MmapKind::File { .. } => {
                let Some(backing) = file_backing else {
                    return Err(MmError::InvalidAddress);
                };
                self.mmap_file(allocator, req, backing)
            }
        }
    }

    fn munmap<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                 allocator : &mut A,
                                                                 addr : VirtAddr,
                                                                 len : usize)
                                                                 -> MmResult<()> {
        if len == 0 {
            return Err(MmError::InvalidAddress);
        }
        let end = VirtAddr(addr.0
                               .checked_add(len)
                               .ok_or(MmError::InvalidAddress)?);
        self.unmap_range_with_alloc(allocator, addr, end)?;
        fence_user_ptes();
        Ok(())
    }

    fn mprotect(&mut self, addr : VirtAddr, len : usize, perm : PagePerm) -> MmResult<()> {
        if len == 0 {
            return Ok(());
        }
        if perm == PagePerm::empty() {
            return Err(MmError::InvalidAddress);
        }
        let end = VirtAddr(addr.0
                               .checked_add(len)
                               .ok_or(MmError::InvalidAddress)?);
        let mut vpn = addr.floor_page();
        let vpn_end = end.ceil_page();
        let perm_u = perm | PagePerm::U;
        while vpn.0 < vpn_end.0 {
            if self.translate_addr(vpn.start_addr())?
                   .is_none()
            {
                return Err(MmError::NotMapped);
            }
            self.protect_page(vpn, perm_u)?;
            vpn = VirtPageNum(vpn.0 + 1);
        }
        fence_user_ptes();
        Ok(())
    }
}
