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
use alloc::boxed::Box;

use api_v0::mmap::{DemandPageLoader, MmapKind, MmapOps, MmapRequest, PageFaultAccess};
use api_v0::perm::PagePerm;
use impl_common::{
    map_range_from_backing, map_range_from_loader, map_zeroed_page_with_alloc,
    map_zeroed_range_with_alloc, mmap_map_end, mremap_range, MREMAP_FIXED,
};

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
        if new_end.0 > r.max.0 {
            return Err(MmError::InvalidAddress);
        }
        if new_end.0 > r.current_end.0 {
            // Expand brk: allocate and map new pages
            let end_vpn_excl = VirtAddr(new_end.0).ceil_page()
                                                  .0;
            let mut vpn_i = VirtAddr(r.current_end.0).floor_page()
                                                     .0;
            while vpn_i < end_vpn_excl {
                let vpn = VirtPageNum(vpn_i);
                let page_start = vpn.start_addr();
                let page_end = VirtPageNum(vpn.0 + 1).start_addr();
                if self.range_overlaps_stack(page_start, page_end) ||
                   self.range_overlaps_kernel_reserved(page_start, page_end) ||
                   self.lazy_vma_overlaps(page_start, page_end)
                {
                    return Err(MmError::InvalidAddress);
                }
                if self.translate_addr(vpn.start_addr())?
                       .is_none()
                {
                    map_zeroed_page_with_alloc(self, allocator, vpn, Self::brk_perm())?;
                }
                vpn_i += 1;
            }
        } else if new_end.0 < r.current_end.0 {
            // Shrink brk: unmap pages that fall outside the new end
            let new_end_vpn_excl = VirtAddr(new_end.0).ceil_page()
                                                      .0;
            let cur_end_vpn_excl = VirtAddr(r.current_end.0).ceil_page()
                                                            .0;
            let mut vpn_i = new_end_vpn_excl;
            while vpn_i < cur_end_vpn_excl {
                let vpn = VirtPageNum(vpn_i);
                if self.translate_addr(vpn.start_addr())?
                       .is_some()
                {
                    self.unmap_page_with_alloc(allocator, vpn)?;
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
    fn handle_stack_page_fault<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                                   allocator : &mut A,
                                                                                   fault_addr : VirtAddr,
                                                                                   access : PageFaultAccess)
                                                                                   -> MmResult<bool> {
        if matches!(access, PageFaultAccess::Execute) {
            return Ok(false);
        }
        if fault_addr.0 < self.user_stack_bottom.0 || fault_addr.0 >= self.user_stack_top.0 {
            return Ok(false);
        }
        let vpn = fault_addr.floor_page();
        if self.translate_addr(vpn.start_addr())?
               .is_some()
        {
            return Ok(true);
        }
        map_zeroed_page_with_alloc(self,
                                   allocator,
                                   vpn,
                                   PagePerm::R | PagePerm::W | PagePerm::U)?;
        fence_user_ptes();
        Ok(true)
    }

    fn handle_brk_page_fault<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                                 allocator : &mut A,
                                                                                 fault_addr : VirtAddr,
                                                                                 access : PageFaultAccess)
                                                                                 -> MmResult<bool> {
        if matches!(access, PageFaultAccess::Execute) {
            return Ok(false);
        }
        if fault_addr.0 < self.user_brk_start.0 || fault_addr.0 >= self.user_brk_current_end.0 {
            return Ok(false);
        }
        let vpn = fault_addr.floor_page();
        let page_end = VirtPageNum(vpn.0 + 1).start_addr();
        if self.lazy_vma_overlaps(vpn.start_addr(), page_end) {
            return Ok(false);
        }
        if self.translate_addr(vpn.start_addr())?
               .is_some()
        {
            return Ok(true);
        }
        map_zeroed_page_with_alloc(self, allocator, vpn, Self::brk_perm())?;
        fence_user_ptes();
        Ok(true)
    }

    fn mmap_anonymous<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                         allocator : &mut A,
                                                                         req : MmapRequest)
                                                                         -> MmResult<VirtAddr> {
        if !req.flags.contains(MapFlags::ANONYMOUS) {
            return Err(MmError::InvalidAddress);
        }
        let shared = req.flags.contains(MapFlags::SHARED);
        let private = req.flags.contains(MapFlags::PRIVATE);
        if !shared && !private {
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
            None => self.find_free_mmap_base_considering_vmas(self.mmap_anon_cursor, req.len)?,
        };
        let end = mmap_map_end(base, req.len)?;
        self.validate_user_mapping_range(base, end)?;
        let perm = req.prot | PagePerm::U;
        if req.flags
              .contains(MapFlags::FIXED)
        {
            self.unmap_range_with_alloc(allocator, base, end)?;
            self.remove_lazy_file_vmas(base, end)?;
            self.remove_shared_anon_vmas(base, end);
        }
        if shared {
            // 共享匿名映射需要稳定的物理帧供 fork 共享，保持饥渴分配。
            map_zeroed_range_with_alloc(self, allocator, base, end, perm)?;
            self.register_shared_anon_vma(base, end);
        } else {
            // 私有匿名映射改为按需零页：仅登记 lazy VMA，缺页时再分配单页，
            // 避免大段栈/堆映射一次性耗尽物理帧。
            self.register_lazy_file_vma(base,
                                        end,
                                        perm,
                                        0,
                                        0,
                                        Box::new(impl_common::ZeroAnonLoader))?;
        }
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
            None => self.find_free_mmap_base_considering_vmas(self.mmap_file_cursor, req.len)?,
        };
        let end = mmap_map_end(base, req.len)?;
        self.validate_user_mapping_range(base, end)?;
        let perm = req.prot | PagePerm::U;
        if req.flags
              .contains(MapFlags::FIXED)
        {
            self.unmap_range_with_alloc(allocator, base, end)?;
        }
        map_range_from_backing(self,
                               allocator,
                               base,
                               end,
                               perm,
                               file_backing)?;
        if req.addr_hint
              .is_none()
        {
            self.mmap_file_cursor = end;
        }
        Ok(base)
    }

    fn mmap_file_with_loader_inner<A, F>(&mut self,
                                         allocator : &mut A,
                                         req : MmapRequest,
                                         load_page : F)
                                         -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>,
              F : FnMut(usize, &mut [u8]) -> MmResult<()>
    {
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
        let base = match req.addr_hint {
            Some(hint)
                if req.flags
                      .contains(MapFlags::FIXED) =>
            {
                hint
            }
            Some(_) => return Err(MmError::InvalidAddress),
            None => self.find_free_mmap_base_considering_vmas(self.mmap_file_cursor, req.len)?,
        };
        let end = mmap_map_end(base, req.len)?;
        self.validate_user_mapping_range(base, end)?;
        let perm = req.prot | PagePerm::U;
        if req.flags
              .contains(MapFlags::FIXED)
        {
            self.unmap_range_with_alloc(allocator, base, end)?;
            self.remove_shared_anon_vmas(base, end);
        }
        map_range_from_loader(self, allocator, base, end, perm, load_page)?;
        if req.flags.contains(MapFlags::SHARED) {
            self.register_shared_anon_vma(base, end);
        }
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

    fn mmap_file_with_loader<A, F>(&mut self,
                                   allocator : &mut A,
                                   req : MmapRequest,
                                   load_page : F)
                                   -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>,
              F : FnMut(usize, &mut [u8]) -> MmResult<()>
    {
        if req.len == 0 {
            return Err(MmError::InvalidAddress);
        }
        match req.kind {
            MmapKind::File { .. } => self.mmap_file_with_loader_inner(allocator, req, load_page),
            MmapKind::Anonymous => Err(MmError::InvalidAddress),
        }
    }

    fn mmap_file_lazy<A>(&mut self,
                         allocator : &mut A,
                         req : MmapRequest,
                         file_size : usize,
                         loader : Box<dyn DemandPageLoader>)
                         -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        let _ = allocator;
        if req.len == 0 {
            return Err(MmError::InvalidAddress);
        }
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
        let base = match req.addr_hint {
            Some(hint)
                if req.flags
                      .contains(MapFlags::FIXED) =>
            {
                hint
            }
            Some(_) => return Err(MmError::InvalidAddress),
            None => self.find_free_mmap_base_considering_vmas(self.mmap_file_cursor, req.len)?,
        };
        let end = mmap_map_end(base, req.len)?;
        self.validate_user_mapping_range(base, end)?;
        let perm = req.prot | PagePerm::U;
        if req.flags
              .contains(MapFlags::FIXED)
        {
            self.unmap_range_with_alloc(allocator, base, end)?;
            self.remove_lazy_file_vmas(base, end)?;
        }
        let file_offset = match req.kind {
            MmapKind::File { offset, .. } => offset,
            MmapKind::Anonymous => return Err(MmError::InvalidAddress),
        };
        self.register_lazy_file_vma(base, end, perm, file_offset, file_size, loader)?;
        if req.addr_hint
              .is_none()
        {
            self.mmap_file_cursor = end;
        }
        Ok(base)
    }

    fn handle_page_fault<A>(&mut self,
                            allocator : &mut A,
                            fault_addr : VirtAddr,
                            access : PageFaultAccess)
                            -> MmResult<bool>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        if self.handle_stack_page_fault(allocator, fault_addr, access)? {
            return Ok(true);
        }
        if self.handle_brk_page_fault(allocator, fault_addr, access)? {
            return Ok(true);
        }
        self.handle_lazy_page_fault(allocator, fault_addr, access)
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
        if end.0 > crate::pagetable::USER_VA_LIMIT ||
           self.range_overlaps_kernel_reserved(addr, end)
        {
            return Err(MmError::InvalidAddress);
        }
        self.unmap_range_with_alloc(allocator, addr, end)?;
        self.remove_lazy_file_vmas(addr.floor_page()
                                       .start_addr(),
                                   end.ceil_page()
                                      .start_addr())?;
        self.remove_shared_anon_vmas(addr.floor_page()
                                         .start_addr(),
                                     end.ceil_page()
                                        .start_addr());
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
        if end.0 > crate::pagetable::USER_VA_LIMIT ||
           self.range_overlaps_kernel_reserved(addr, end)
        {
            return Err(MmError::InvalidAddress);
        }
        let perm_u = perm | PagePerm::U;
        self.protect_lazy_file_vmas(addr.floor_page()
                                        .start_addr(),
                                    end.ceil_page()
                                       .start_addr(),
                                    perm_u)?;
        let mut vpn = addr.floor_page();
        let vpn_end = end.ceil_page();
        while vpn.0 < vpn_end.0 {
            if self.translate_addr(vpn.start_addr())?
                   .is_none()
            {
                let page_end = VirtPageNum(vpn.0 + 1).start_addr();
                if self.lazy_vma_overlaps(vpn.start_addr(), page_end) {
                    vpn = VirtPageNum(vpn.0 + 1);
                    continue;
                }
                return Err(MmError::NotMapped);
            }
            self.protect_page(vpn, perm_u)?;
            vpn = VirtPageNum(vpn.0 + 1);
        }
        fence_user_ptes();
        Ok(())
    }

    fn mremap<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                 allocator : &mut A,
                                                                 old_addr : VirtAddr,
                                                                 old_size : usize,
                                                                 new_size : usize,
                                                                 flags : usize,
                                                                 new_address : VirtAddr)
                                                                 -> MmResult<VirtAddr> {
        let old_end = mmap_map_end(old_addr, old_size)?;
        if old_end.0 > crate::pagetable::USER_VA_LIMIT ||
           self.range_overlaps_stack(old_addr, old_end) ||
           self.range_overlaps_kernel_reserved(old_addr, old_end)
        {
            return Err(MmError::InvalidAddress);
        }
        if flags & MREMAP_FIXED != 0 {
            let end = mmap_map_end(new_address, new_size)?;
            self.validate_user_mapping_range(new_address, end)?;
        } else {
            let end = mmap_map_end(old_addr, new_size)?;
            if end.0 > crate::pagetable::USER_VA_LIMIT ||
               self.range_overlaps_stack(old_addr, end) ||
               self.range_overlaps_kernel_reserved(old_addr, end)
            {
                return Err(MmError::InvalidAddress);
            }
        }
        mremap_range(self,
                     allocator,
                     old_addr,
                     old_size,
                     new_size,
                     flags,
                     new_address,
                     self.mmap_anon_cursor)
    }
}

impl LoongArch64AddressSpace {
    pub fn madvise_discard_mapped_pages<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator : &mut A,
        addr : VirtAddr,
        len : usize,
    ) -> MmResult<()> {
        if len == 0 {
            return Ok(());
        }
        let end = VirtAddr(addr.0
                               .checked_add(len)
                               .ok_or(MmError::InvalidAddress)?);
        let mut vpn = addr.floor_page();
        let vpn_end = end.ceil_page();
        while vpn.0 < vpn_end.0 {
            if self.translate_addr(vpn.start_addr())?
                   .is_some()
            {
                self.unmap_page_with_alloc(allocator, vpn)?;
            }
            vpn = VirtPageNum(vpn.0 + 1);
        }
        fence_user_ptes();
        Ok(())
    }
}
