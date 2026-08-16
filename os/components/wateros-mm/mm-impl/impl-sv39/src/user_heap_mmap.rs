//! 本模块代码由AI完成
//! 用户堆 `brk` 与匿名/文件 `mmap`/`munmap`/`mprotect` 的 Sv39 实现。
//!
//! # 缺页与 trap
//!
//! 与 [`crate::pagetable`] 一致：映射 **饥渴（eager）** 建立；未映射用户 VA 由硬件 page fault
//! 进入 trap，本阶段不提供 demand paging。

use alloc::{boxed::Box, vec::Vec};
use api_v0::addr::{PhysPageNum, VirtAddr, VirtPageNum};
use api_v0::address_space::AddressSpaceOps;
use api_v0::brk::{BrkRegion, HeapBrk};
use api_v0::error::{MmError, MmResult};
use api_v0::flags::MapFlags;
use api_v0::frame_allocator::PhysicalFrameAllocator;

use api_v0::mmap::{
    DemandPageLoader, DeviceMapping, MmapKind, MmapOps, MmapRequest, PageFaultAccess,
};
use api_v0::perm::PagePerm;
use impl_common::{
    map_range_from_backing, map_range_from_loader, map_zeroed_page_with_alloc,
    map_zeroed_range_with_alloc, mmap_map_end, mremap_range, MREMAP_FIXED, MREMAP_MAYMOVE,
};

use crate::pagetable::{DeviceVma, LazyFileVma, Sv39AddressSpace, VmaBacking};

#[inline]
fn fence_user_ptes() { platform::arch::paging::flush_address_space_translations(); }

impl HeapBrk for Sv39AddressSpace {
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
            // 堆向上增长：为新增虚拟页分配并映射零页
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
            // 堆收缩：解除新边界之外的已映射页
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

impl Sv39AddressSpace {
    fn mmap_device_inner<A>(&mut self,
                            allocator : &mut A,
                            req : MmapRequest,
                            mapping : DeviceMapping)
                            -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        if req.len == 0 ||
           !req.flags
               .contains(MapFlags::SHARED) ||
           req.flags
              .contains(MapFlags::PRIVATE) ||
           req.flags
              .contains(MapFlags::ANONYMOUS) ||
           req.prot
              .executable()
        {
            return Err(MmError::InvalidAddress);
        }
        let offset = match req.kind {
            MmapKind::Device { offset } => offset,
            _ => return Err(MmError::InvalidAddress),
        };
        if offset % api_v0::addr::PAGE_SIZE != 0 || mapping.len == 0 {
            return Err(MmError::InvalidAddress);
        }
        let rounded_len = req.len
                             .checked_add(api_v0::addr::PAGE_SIZE - 1)
                             .ok_or(MmError::InvalidAddress)? /
                          api_v0::addr::PAGE_SIZE *
                          api_v0::addr::PAGE_SIZE;
        if offset.checked_add(rounded_len)
                 .ok_or(MmError::InvalidAddress)? >
           mapping.len
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
        if req.flags
              .contains(MapFlags::FIXED)
        {
            self.sync_shared_file_vmas(base, end)?;
            self.unmap_mmap_range(allocator, base, end)?;
            self.remove_lazy_file_vmas(base, end)?;
            self.remove_shared_file_vmas(base, end)?;
            self.remove_shared_anon_vmas(base, end);
            self.remove_device_vmas(base, end);
        }
        let perm = req.prot | PagePerm::U;
        let phys_start = PhysPageNum(mapping.phys_start.0 + offset / api_v0::addr::PAGE_SIZE);
        let mut vpn = base.floor_page();
        let vpn_end = end.ceil_page();
        let mut page_index = 0usize;
        while vpn.0 < vpn_end.0 {
            if let Err(error) = self.map_page_to_ppn(vpn,
                                                     PhysPageNum(phys_start.0 + page_index),
                                                     perm)
            {
                let mut rollback = base.floor_page();
                while rollback.0 < vpn.0 {
                    let _ = self.unmap_page_to_ppn(rollback);
                    rollback = VirtPageNum(rollback.0 + 1);
                }
                return Err(error);
            }
            vpn = VirtPageNum(vpn.0 + 1);
            page_index += 1;
        }
        self.register_device_vma(DeviceVma { start : base,
                                             end,
                                             phys_start,
                                             perm,
                                             lease : mapping.lease });
        if req.addr_hint
              .is_none()
        {
            self.mmap_file_cursor = end;
        }
        fence_user_ptes();
        Ok(base)
    }

    fn handle_stack_page_fault<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator : &mut A,
        fault_addr : VirtAddr,
        access : PageFaultAccess)
        -> MmResult<bool> {
        if matches!(access, PageFaultAccess::Execute) {
            return Ok(false);
        }
        if fault_addr.0 <
           self.user_stack_bottom
               .0 ||
           fault_addr.0 >=
           self.user_stack_top
               .0
        {
            return Ok(false);
        }
        let vpn = fault_addr.floor_page();
        if self.translate_addr(vpn.start_addr())?
               .is_some()
        {
            platform::arch::paging::flush_tlb_local(
                platform::arch::paging::TlbFlushRange::Page { addr : vpn.start_addr().0 });
            return Ok(true);
        }
        map_zeroed_page_with_alloc(self,
                                   allocator,
                                   vpn,
                                   PagePerm::R | PagePerm::W | PagePerm::U)?;
        platform::arch::paging::flush_tlb_local(
            platform::arch::paging::TlbFlushRange::Page { addr : vpn.start_addr().0 });
        Ok(true)
    }

    fn handle_brk_page_fault<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator : &mut A,
        fault_addr : VirtAddr,
        access : PageFaultAccess)
        -> MmResult<bool> {
        if matches!(access, PageFaultAccess::Execute) {
            return Ok(false);
        }
        if fault_addr.0 <
           self.user_brk_start
               .0 ||
           fault_addr.0 >=
           self.user_brk_current_end
               .0
        {
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
            platform::arch::paging::flush_tlb_local(
                platform::arch::paging::TlbFlushRange::Page { addr : vpn.start_addr().0 });
            return Ok(true);
        }
        map_zeroed_page_with_alloc(self, allocator, vpn, Self::brk_perm())?;
        platform::arch::paging::flush_tlb_local(
            platform::arch::paging::TlbFlushRange::Page { addr : vpn.start_addr().0 });
        Ok(true)
    }

    fn mmap_anonymous<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                         allocator : &mut A,
                                                                         req : MmapRequest)
                                                                         -> MmResult<VirtAddr> {
        if !req.flags
               .contains(MapFlags::ANONYMOUS)
        {
            return Err(MmError::InvalidAddress);
        }
        let shared = req.flags
                        .contains(MapFlags::SHARED);
        let private = req.flags
                         .contains(MapFlags::PRIVATE);
        if !shared && !private {
            return Err(MmError::InvalidAddress);
        }
        let fixed = req.flags
                       .contains(MapFlags::FIXED);
        let fixed_noreplace = req.flags
                                 .contains(MapFlags::FIXED_NOREPLACE);
        if fixed && fixed_noreplace {
            return Err(MmError::InvalidAddress);
        }
        let base = match req.addr_hint {
            Some(hint) if fixed || fixed_noreplace => hint,
            Some(_) => return Err(MmError::InvalidAddress),
            None => self.find_free_mmap_base_considering_vmas(self.mmap_anon_cursor, req.len)?,
        };
        let end = mmap_map_end(base, req.len)?;
        self.validate_user_mapping_range(base, end)?;
        if fixed_noreplace && self.user_mapping_range_occupied(base, end)? {
            return Err(MmError::InvalidAddress);
        }
        let perm = req.prot | PagePerm::U;
        if fixed {
            self.unmap_mmap_range(allocator, base, end)?;
            self.remove_lazy_file_vmas(base, end)?;
            self.remove_shared_anon_vmas(base, end);
            self.remove_shared_file_vmas(base, end)?;
            self.remove_device_vmas(base, end);
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
                                        VmaBacking::Anonymous)?;
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
            self.unmap_mmap_range(allocator, base, end)?;
            self.remove_lazy_file_vmas(base, end)?;
            self.remove_shared_anon_vmas(base, end);
            self.remove_shared_file_vmas(base, end)?;
            self.remove_device_vmas(base, end);
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

    fn mmap_file_shared_inner<A>(&mut self,
                                 allocator : &mut A,
                                 req : MmapRequest,
                                 mut loader : Box<dyn DemandPageLoader>)
                                 -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        if req.flags
              .contains(MapFlags::ANONYMOUS)
        {
            return Err(MmError::InvalidAddress);
        }
        if !req.flags
               .contains(MapFlags::SHARED)
        {
            return Err(MmError::InvalidAddress);
        }
        let file_offset = match req.kind {
            MmapKind::File { offset, .. } => offset,
            MmapKind::Anonymous | MmapKind::Device { .. } => {
                return Err(MmError::InvalidAddress);
            }
        };
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
            self.sync_shared_file_vmas(base, end)?;
            self.unmap_mmap_range(allocator, base, end)?;
            self.remove_shared_file_vmas(base, end)?;
            self.remove_shared_anon_vmas(base, end);
            self.remove_lazy_file_vmas(base, end)?;
            self.remove_device_vmas(base, end);
        }
        map_range_from_loader(self,
                              allocator,
                              base,
                              end,
                              perm,
                              |page_index, page| {
                                  let offset = file_offset.checked_add(page_index.checked_mul(api_v0::addr::PAGE_SIZE)
                                                           .ok_or(MmError::InvalidAddress)?)
                                    .ok_or(MmError::InvalidAddress)?;
                                  loader.load_page(offset, page)
                              })?;
        self.register_shared_anon_vma(base, end);
        self.register_shared_file_vma(base, end, file_offset, loader);
        if req.addr_hint
              .is_none()
        {
            self.mmap_file_cursor = end;
        }
        Ok(base)
    }
}

impl MmapOps for Sv39AddressSpace {
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
            MmapKind::Device { .. } => Err(MmError::InvalidAddress),
        }
    }

    fn mmap_file_shared<A>(&mut self,
                           allocator : &mut A,
                           req : MmapRequest,
                           loader : Box<dyn DemandPageLoader>)
                           -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        if req.len == 0 {
            return Err(MmError::InvalidAddress);
        }
        match req.kind {
            MmapKind::File { .. } => self.mmap_file_shared_inner(allocator, req, loader),
            MmapKind::Anonymous | MmapKind::Device { .. } => Err(MmError::InvalidAddress),
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
            self.unmap_mmap_range(allocator, base, end)?;
            self.remove_lazy_file_vmas(base, end)?;
            self.remove_shared_anon_vmas(base, end);
            self.remove_shared_file_vmas(base, end)?;
            self.remove_device_vmas(base, end);
        }
        let file_offset = match req.kind {
            MmapKind::File { offset, .. } => offset,
            MmapKind::Anonymous | MmapKind::Device { .. } => {
                return Err(MmError::InvalidAddress);
            }
        };
        self.register_lazy_file_vma(base,
                                    end,
                                    perm,
                                    file_offset,
                                    file_size,
                                    VmaBacking::File { loader })?;
        if req.addr_hint
              .is_none()
        {
            self.mmap_file_cursor = end;
        }
        Ok(base)
    }

    fn mmap_device<A>(&mut self,
                      allocator : &mut A,
                      req : MmapRequest,
                      mapping : DeviceMapping)
                      -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        self.mmap_device_inner(allocator, req, mapping)
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
        if end.0 > crate::pagetable::USER_VA_LIMIT || self.range_overlaps_kernel_reserved(addr, end)
        {
            return Err(MmError::InvalidAddress);
        }
        let page_start = addr.floor_page()
                             .start_addr();
        let page_end = end.ceil_page()
                          .start_addr();
        self.sync_shared_file_vmas(page_start, page_end)?;
        self.unmap_mmap_range(allocator, addr, end)?;
        self.remove_lazy_file_vmas(addr.floor_page()
                                       .start_addr(),
                                   end.ceil_page()
                                      .start_addr())?;
        self.remove_shared_anon_vmas(addr.floor_page()
                                         .start_addr(),
                                     end.ceil_page()
                                        .start_addr());
        self.remove_shared_file_vmas(page_start, page_end)?;
        self.remove_device_vmas(page_start, page_end);
        fence_user_ptes();
        Ok(())
    }

    fn munmap_external(&mut self, addr : VirtAddr, len : usize) -> MmResult<()> {
        if len == 0 {
            return Err(MmError::InvalidAddress);
        }
        let end = VirtAddr(addr.0
                               .checked_add(len)
                               .ok_or(MmError::InvalidAddress)?);
        if end.0 > crate::pagetable::USER_VA_LIMIT || self.range_overlaps_kernel_reserved(addr, end)
        {
            return Err(MmError::InvalidAddress);
        }
        let page_start = addr.floor_page()
                             .start_addr();
        let page_end = end.ceil_page()
                          .start_addr();
        let mut vpn = page_start.floor_page();
        let vpn_end = page_end.floor_page();
        while vpn.0 < vpn_end.0 {
            // 外部对象仍持有 PPN；这里故意丢弃返回值而不调用 frame_dealloc。
            let _ = self.unmap_page_to_ppn(vpn)?;
            vpn = VirtPageNum(vpn.0 + 1);
        }
        self.remove_lazy_file_vmas(page_start, page_end)?;
        self.remove_shared_anon_vmas(page_start, page_end);
        self.remove_shared_file_vmas(page_start, page_end)?;
        self.remove_device_vmas(page_start, page_end);
        fence_user_ptes();
        Ok(())
    }

    fn msync(&mut self, addr : VirtAddr, len : usize) -> MmResult<()> {
        if len == 0 {
            return Ok(());
        }
        let end = VirtAddr(addr.0
                               .checked_add(len)
                               .ok_or(MmError::InvalidAddress)?);
        self.sync_shared_file_vmas(addr.floor_page()
                                       .start_addr(),
                                   end.ceil_page()
                                      .start_addr())
    }

    fn mprotect(&mut self, addr : VirtAddr, len : usize, perm : PagePerm) -> MmResult<bool> {
        if len == 0 {
            return Ok(false);
        }
        let end = VirtAddr(addr.0
                               .checked_add(len)
                               .ok_or(MmError::InvalidAddress)?);
        if end.0 > crate::pagetable::USER_VA_LIMIT || self.range_overlaps_kernel_reserved(addr, end)
        {
            return Err(MmError::InvalidAddress);
        }
        let page_start = addr.floor_page()
                             .start_addr();
        let page_end = end.ceil_page()
                          .start_addr();
        if perm.executable() && self.device_vma_overlaps(page_start, page_end) {
            return Err(MmError::AccessViolation);
        }
        let perm_u = perm | PagePerm::U;
        self.protect_lazy_file_vmas(addr.floor_page()
                                        .start_addr(),
                                    end.ceil_page()
                                       .start_addr(),
                                    perm_u)?;
        let mut vpn = addr.floor_page();
        let vpn_end = end.ceil_page();
        let mut ptes_changed = false;
        while vpn.0 < vpn_end.0 {
            let Some(old_pa) = self.translate_addr(vpn.start_addr())? else {
                let page_end = VirtPageNum(vpn.0 + 1).start_addr();
                if self.lazy_vma_overlaps(vpn.start_addr(), page_end) {
                    vpn = VirtPageNum(vpn.0 + 1);
                    continue;
                }
                return Err(MmError::NotMapped);
            };
            let old_perm = self.leaf_page_perm(vpn)?
                               .ok_or(MmError::NotMapped)?;
            let device_page = self.device_vmas
                                  .iter()
                                  .any(|vma| vma.contains_page(vpn.start_addr()));
            if perm_u.writable() && !device_page {
                if !self.ensure_private_for_write(vpn)? {
                    return Err(MmError::NotMapped);
                }
                let new_pa = self.translate_addr(vpn.start_addr())?
                                 .ok_or(MmError::NotMapped)?;
                ptes_changed |= new_pa.floor_page() != old_pa.floor_page();
            }
            if old_perm != perm_u {
                self.protect_page(vpn, perm_u)?;
                ptes_changed = true;
            }
            vpn = VirtPageNum(vpn.0 + 1);
        }
        self.protect_device_vmas(page_start, page_end, perm_u);
        Ok(ptes_changed)
    }

    fn mremap<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                 allocator : &mut A,
                                                                 old_addr : VirtAddr,
                                                                 old_size : usize,
                                                                 new_size : usize,
                                                                 flags : usize,
                                                                 new_address : VirtAddr)
                                                                 -> MmResult<VirtAddr> {
        if old_addr.0 % api_v0::addr::PAGE_SIZE != 0 || old_size == 0 {
            return Err(MmError::InvalidAddress);
        }
        let old_start = old_addr.floor_page()
                                .start_addr();
        let old_end = mmap_map_end(old_addr, old_size)?;
        if self.device_vma_overlaps(old_start, old_end) {
            return Err(MmError::Unsupported);
        }
        if old_end.0 > crate::pagetable::USER_VA_LIMIT ||
           self.range_overlaps_stack(old_addr, old_end) ||
           self.range_overlaps_kernel_reserved(old_addr, old_end)
        {
            return Err(MmError::InvalidAddress);
        }
        if flags & MREMAP_FIXED != 0 {
            let end = mmap_map_end(new_address, new_size)?;
            self.validate_user_mapping_range(new_address, end)?;
            if new_address.0 < old_end.0 && end.0 > old_start.0 {
                return Err(MmError::InvalidAddress);
            }
            if self.device_vma_overlaps(new_address, end) {
                self.unmap_mmap_range(allocator, new_address, end)?;
                self.remove_device_vmas(new_address, end);
            }
        } else {
            let end = mmap_map_end(old_addr, new_size)?;
            if end.0 > crate::pagetable::USER_VA_LIMIT ||
               self.range_overlaps_stack(old_addr, end) ||
               self.range_overlaps_kernel_reserved(old_addr, end)
            {
                return Err(MmError::InvalidAddress);
            }
        }
        let lazy_overlap = self.lazy_vma_overlaps(old_start, old_end);
        let lazy_vma =
            self.lazy_file_vmas
                .iter()
                .find(|vma| vma.start.0 == old_start.0 && vma.end.0 == old_end.0)
                .map(|vma| {
                    Ok::<LazyFileVma, MmError>(LazyFileVma { start : vma.start,
                                                             end : vma.end,
                                                             perm : vma.perm,
                                                             file_offset : vma.file_offset,
                                                             file_size : vma.file_size,
                                                             backing : vma.backing
                                                                          .duplicate()? })
                })
                .transpose()?;
        if lazy_overlap && lazy_vma.is_none() {
            return Err(MmError::Unsupported);
        }
        // Copying a shared mapping would break its physical-page identity.
        if self.shared_anon_vma_overlaps(old_start, old_end) {
            return Err(MmError::Unsupported);
        }
        let perm = match lazy_vma.as_ref() {
            Some(vma) => vma.perm,
            None => {
                let perm = self.leaf_page_perm(old_start.floor_page())?
                               .ok_or(MmError::NotMapped)?;
                let mut vpn = old_start.floor_page();
                let vpn_end = old_end.ceil_page();
                while vpn.0 < vpn_end.0 {
                    if self.leaf_page_perm(vpn)? != Some(perm) {
                        return Err(MmError::Unsupported);
                    }
                    vpn = VirtPageNum(vpn.0 + 1);
                }
                perm
            }
        };
        let requested_end = mmap_map_end(old_addr, new_size)?;
        let mut force_move = flags & MREMAP_FIXED == 0 &&
                             requested_end.0 > old_end.0 &&
                             (self.lazy_vma_overlaps(old_end, requested_end) ||
                              self.shared_anon_vma_overlaps(old_end, requested_end));
        if flags & MREMAP_FIXED == 0 && requested_end.0 > old_end.0 && !force_move {
            let mut vpn = old_end.floor_page();
            let vpn_end = requested_end.ceil_page();
            while vpn.0 < vpn_end.0 {
                if self.translate_addr(vpn.start_addr())?
                       .is_some()
                {
                    force_move = true;
                    break;
                }
                vpn = VirtPageNum(vpn.0 + 1);
            }
        }
        let relocation_base = if force_move && flags & MREMAP_MAYMOVE != 0 {
            self.find_free_mmap_base_considering_vmas(self.mmap_anon_cursor, new_size)?
        } else {
            old_addr
        };
        let result = mremap_range(self,
                                  allocator,
                                  old_addr,
                                  old_size,
                                  new_size,
                                  flags,
                                  new_address,
                                  relocation_base,
                                  force_move,
                                  perm)?;
        let result_end = mmap_map_end(result, new_size)?;
        let keep_old = flags & impl_common::MREMAP_DONTUNMAP != 0;
        if let Some(mut vma) = lazy_vma {
            if !keep_old {
                self.remove_lazy_file_vmas(old_start, old_end)?;
            }
            self.remove_lazy_file_vmas(result, result_end)?;
            vma.start = result;
            vma.end = result_end;
            self.register_lazy_file_vma(vma.start,
                                        vma.end,
                                        vma.perm,
                                        vma.file_offset,
                                        vma.file_size,
                                        vma.backing)?;
        } else if flags & MREMAP_FIXED != 0 {
            self.remove_lazy_file_vmas(result, result_end)?;
        }
        if flags & MREMAP_FIXED != 0 {
            self.remove_shared_anon_vmas(result, result_end);
        }
        Ok(result)
    }
}

impl Sv39AddressSpace {
    /// 将当前地址空间中所有可按需装入的 VMA 变为驻留。
    ///
    /// eager mmap、共享映射和设备映射在创建时已有 PTE；这里只需覆盖文件
    /// lazy VMA、brk 当前区间和用户栈保留区。先复制轻量元数据，避免在调用
    /// `handle_page_fault` 时仍借用 VMA 数组。
    pub fn prefault_all_current_user_ranges<A>(&mut self, allocator : &mut A) -> MmResult<()>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum> {
        let mut ranges : Vec<(VirtAddr, VirtAddr, PageFaultAccess)> =
            self.lazy_file_vmas
                .iter()
                .filter_map(|vma| {
                    let access = if vma.perm.writable() {
                        PageFaultAccess::Write
                    } else if vma.perm.readable() {
                        PageFaultAccess::Read
                    } else if vma.perm
                                 .executable()
                    {
                        PageFaultAccess::Execute
                    } else {
                        // PROT_NONE 区域没有可执行的用户访问，也没有已驻留页需要补。
                        return None;
                    };
                    Some((vma.start, vma.end, access))
                })
                .collect();
        if self.user_brk_start
               .0 <
           self.user_brk_current_end
               .0
        {
            ranges.push((self.user_brk_start, self.user_brk_current_end, PageFaultAccess::Read));
        }
        if self.user_stack_bottom
               .0 <
           self.user_stack_top
               .0
        {
            ranges.push((self.user_stack_bottom, self.user_stack_top, PageFaultAccess::Read));
        }
        for (start, end, access) in ranges {
            let mut vpn = start.floor_page();
            let vpn_end = end.ceil_page();
            while vpn.0 < vpn_end.0 {
                let page = vpn.start_addr();
                if self.translate_addr(page)?
                       .is_none() &&
                   !MmapOps::handle_page_fault(self, allocator, page, access)?
                {
                    return Err(MmError::InvalidAddress);
                }
                vpn = VirtPageNum(vpn.0 + 1);
            }
        }
        Ok(())
    }

    pub fn madvise_range_mapped(&self, addr : VirtAddr, len : usize) -> bool {
        if len == 0 {
            return true;
        }
        let Some(end) = addr.0
                            .checked_add(len)
                            .map(VirtAddr)
        else {
            return false;
        };
        let mut vpn = addr.floor_page();
        let vpn_end = end.ceil_page();
        while vpn.0 < vpn_end.0 {
            let page = vpn.start_addr();
            if self.translate_addr(page)
                   .ok()
                   .flatten()
                   .is_some()
            {
                vpn = VirtPageNum(vpn.0 + 1);
                continue;
            }
            let in_vma = self.lazy_file_vmas
                             .iter()
                             .any(|vma| vma.contains_page(page)) ||
                         self.shared_anon_vmas
                             .iter()
                             .any(|vma| vma.contains_page(page)) ||
                         self.shared_file_vmas
                             .iter()
                             .any(|vma| vma.start.0 <= page.0 && page.0 < vma.end.0) ||
                         self.device_vmas
                             .iter()
                             .any(|vma| vma.contains_page(page));
            let in_stack = self.user_stack_bottom
                               .0 <=
                           page.0 &&
                           page.0 <
                           self.user_stack_top
                               .0;
            let in_brk = self.user_brk_start
                             .0 <=
                         page.0 &&
                         page.0 <
                         self.user_brk_current_end
                             .0;
            if !in_vma && !in_stack && !in_brk {
                return false;
            }
            vpn = VirtPageNum(vpn.0 + 1);
        }
        true
    }

    pub fn madvise_range_shared_or_file(&self, addr : VirtAddr, len : usize) -> bool {
        if len == 0 {
            return false;
        }
        let Some(end) = addr.0
                            .checked_add(len)
                            .map(VirtAddr)
        else {
            return true;
        };
        self.lazy_file_vmas
            .iter()
            .any(|vma| vma.overlaps(addr, end)) ||
        self.shared_file_vmas
            .iter()
            .any(|vma| vma.overlaps(addr, end)) ||
        self.shared_anon_vmas
            .iter()
            .any(|vma| vma.overlaps(addr, end)) ||
        self.device_vmas
            .iter()
            .any(|vma| vma.overlaps(addr, end))
    }

    /// `MADV_DONTNEED` / `MADV_FREE`：丢弃已映射用户页，保留 lazy VMA 以便再次 fault。
    pub fn madvise_discard_mapped_pages<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator : &mut A,
        addr : VirtAddr,
        len : usize)
        -> MmResult<()> {
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
