//! 用户堆 `brk` 与匿名/文件 `mmap`/`munmap`/`mprotect` 的 LoongArch64 实现。
//!
//! # 缺页与 trap
//!
//! 与 [`crate::pagetable`] 一致：私有匿名/文件映射可只登记 VMA 并在 page fault 时按需装页；
//! 共享匿名、SysV SHM 和设备映射 eager 建立驻留 PTE。

use alloc::{boxed::Box, vec::Vec};
use api_v0::addr::{PhysPageNum, VirtAddr, VirtPageNum};
use api_v0::address_space::AddressSpaceOps;
use api_v0::brk::{BrkRegion, HeapBrk};
use api_v0::error::{MmError, MmResult};
use api_v0::flags::MapFlags;
use api_v0::frame_allocator::PhysicalFrameAllocator;

use api_v0::mmap::{
    DemandPageLoader, DeviceMapping, MmapKind, MmapOps, MmapRequest, PageFaultAccess, PteChange,
};
use api_v0::perm::PagePerm;
use impl_common::{
    map_external_pages, map_range_from_backing, map_range_from_loader, map_zeroed_page_with_alloc,
    map_zeroed_range_with_alloc, mmap_map_end, mremap_range, MREMAP_FIXED, MREMAP_MAYMOVE,
};

use crate::pagetable::{LoongArch64AddressSpace, VmArea, VmaBacking, VmaKind, VmaSharing};

#[inline]
fn fence_user_ptes() { platform::arch::paging::flush_address_space_translations(); }

impl HeapBrk for LoongArch64AddressSpace {
    /// 返回 LoongArch 用户堆的半开边界；`current_end` 可非页对齐，实际 PTE 按页管理。
    fn brk_region(&self) -> BrkRegion {
        BrkRegion { start : self.user_brk_start,
                    current_end : self.user_brk_current_end,
                    max : self.user_brk_max }
    }

    fn brk<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                              allocator : &mut A,
                                                              new_end : VirtAddr)
                                                              -> MmResult<VirtAddr> {
        // `brk(0)` 是查询，不改变页表或当前 break。
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
            // 堆向上增长：为新增虚拟页分配并映射零页；先检查栈和 lazy VMA，避免建立重叠映射。
            let end_vpn_excl = VirtAddr(new_end.0).ceil_page()
                                                  .0;
            let mut vpn_i = VirtAddr(r.current_end.0).floor_page()
                                                     .0;
            while vpn_i < end_vpn_excl {
                let vpn = VirtPageNum(vpn_i);
                let page_start = vpn.start_addr();
                let page_end = VirtPageNum(vpn.0 + 1).start_addr();
                if self.vmas
                       .overlaps_where(page_start, page_end, |vma| {
                           vma.kind != VmaKind::Heap
                       })
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
            let mut page = VirtAddr(r.current_end.0).floor_page();
            let page_end = VirtAddr(new_end.0).ceil_page();
            while page.0 < page_end.0 {
                let start = page.start_addr();
                if self.vmas
                       .lookup(start)
                       .is_none()
                {
                    self.vmas
                        .insert(VmArea::anonymous(start,
                                                  VirtPageNum(page.0 + 1).start_addr(),
                                                  Self::brk_perm(),
                                                  VmaSharing::Private,
                                                  VmaKind::Heap,
                                                  true))?;
                }
                page = VirtPageNum(page.0 + 1);
            }
        } else if new_end.0 < r.current_end.0 {
            // 堆收缩：解除新边界之外的已映射页；保留新 break 所在的部分页。
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
            self.remove_vmas(VirtPageNum(new_end_vpn_excl).start_addr(),
                             VirtPageNum(cur_end_vpn_excl).start_addr())?;
        }
        self.user_brk_current_end = new_end;
        fence_user_ptes();
        Ok(new_end)
    }
}

impl LoongArch64AddressSpace {
    /// 校验并建立设备页映射；设备帧由租约管理，不得交给普通帧分配器回收。
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
            self.remove_vmas(base, end)?;
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
        self.register_device_vma(VmArea::device(base,
                                                end,
                                                perm,
                                                phys_start,
                                                mapping.lease))?;
        if req.addr_hint
              .is_none()
        {
            self.mmap_file_cursor = end;
        }
        fence_user_ptes();
        Ok(base)
    }

    /// 建立物理页由外部子系统持有的共享 VMA；当前调用者是 SysV SHM。
    fn mmap_external_inner<A>(&mut self,
                              allocator : &mut A,
                              req : MmapRequest,
                              pages : &[PhysPageNum])
                              -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        let fixed = req.flags
                       .contains(MapFlags::FIXED);
        let fixed_noreplace = req.flags
                                 .contains(MapFlags::FIXED_NOREPLACE);
        if req.len == 0 ||
           fixed && fixed_noreplace ||
           !req.flags
               .contains(MapFlags::ANONYMOUS) ||
           !req.flags
               .contains(MapFlags::SHARED) ||
           req.flags
              .contains(MapFlags::PRIVATE) ||
           !matches!(req.kind, MmapKind::Anonymous)
        {
            return Err(MmError::InvalidAddress);
        }
        let base = match req.addr_hint {
            Some(hint) if fixed || fixed_noreplace => hint,
            Some(_) => return Err(MmError::InvalidAddress),
            None => self.find_free_mmap_base_considering_vmas(self.mmap_anon_cursor, req.len)?,
        };
        if base.0 % api_v0::addr::PAGE_SIZE != 0 {
            return Err(MmError::InvalidAddress);
        }
        let end = mmap_map_end(base, req.len)?;
        self.validate_user_mapping_range(base, end)?;
        if pages.len() != (end.0 - base.0) / api_v0::addr::PAGE_SIZE {
            return Err(MmError::InvalidAddress);
        }
        if fixed_noreplace && self.user_mapping_range_occupied(base, end)? {
            return Err(MmError::AlreadyMapped);
        }
        if fixed {
            self.sync_shared_file_vmas(base, end)?;
            self.unmap_mmap_range(allocator, base, end)?;
            self.remove_vmas(base, end)?;
        }
        let perm = req.prot | PagePerm::U;
        map_external_pages(self, base, end, perm, pages)?;
        if let Err(error) = self.register_external_vma(VmArea::external(base, end, perm)) {
            let mut vpn = base.floor_page();
            let vpn_end = end.floor_page();
            while vpn.0 < vpn_end.0 {
                let _ = self.unmap_page_to_ppn(vpn);
                vpn = VirtPageNum(vpn.0 + 1);
            }
            return Err(error);
        }
        if req.addr_hint
              .is_none()
        {
            self.mmap_anon_cursor = end;
        }
        fence_user_ptes();
        Ok(base)
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
            self.sync_shared_file_vmas(base, end)?;
            self.unmap_mmap_range(allocator, base, end)?;
            self.remove_vmas(base, end)?;
        }
        if shared {
            // 共享匿名映射需要稳定的物理帧供 fork 共享，保持饥渴分配。
            map_zeroed_range_with_alloc(self, allocator, base, end, perm)?;
            self.register_shared_anon_vma(base, end, perm)?;
        } else {
            // 私有匿名映射改为按需零页：仅登记 lazy VMA，缺页时再分配单页，
            // 避免大段栈/堆映射一次性耗尽物理帧。
            self.register_demand_vma(base,
                                     end,
                                     perm,
                                     VmaSharing::Private,
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
            self.sync_shared_file_vmas(base, end)?;
            self.unmap_mmap_range(allocator, base, end)?;
            self.remove_vmas(base, end)?;
        }
        map_range_from_backing(self,
                               allocator,
                               base,
                               end,
                               perm,
                               file_backing)?;
        let sharing = if req.flags
                            .contains(MapFlags::SHARED)
        {
            VmaSharing::Shared
        } else {
            VmaSharing::Private
        };
        let file_offset = match req.kind {
            MmapKind::File { offset, .. } => offset,
            MmapKind::Anonymous | MmapKind::Device { .. } => 0,
        };
        self.vmas
            .insert(VmArea::file(base,
                                 end,
                                 perm,
                                 sharing,
                                 false,
                                 file_offset,
                                 file_backing.len(),
                                 None))?;
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
            self.remove_vmas(base, end)?;
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
        self.register_shared_file_vma(base, end, perm, file_offset, loader)?;
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
            self.sync_shared_file_vmas(base, end)?;
            self.unmap_mmap_range(allocator, base, end)?;
            self.remove_vmas(base, end)?;
        }
        let file_offset = match req.kind {
            MmapKind::File { offset, .. } => offset,
            MmapKind::Anonymous | MmapKind::Device { .. } => {
                return Err(MmError::InvalidAddress);
            }
        };
        self.register_demand_vma(base,
                                 end,
                                 perm,
                                 if req.flags
                                       .contains(MapFlags::SHARED)
                                 {
                                     VmaSharing::Shared
                                 } else {
                                     VmaSharing::Private
                                 },
                                 file_offset,
                                 file_size,
                                 VmaBacking::File { loader : Some(loader) })?;
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

    fn mmap_external<A>(&mut self,
                        allocator : &mut A,
                        req : MmapRequest,
                        pages : &[PhysPageNum])
                        -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        self.mmap_external_inner(allocator, req, pages)
    }

    fn handle_page_fault<A>(&mut self,
                            allocator : &mut A,
                            fault_addr : VirtAddr,
                            access : PageFaultAccess)
                            -> MmResult<bool>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        self.handle_vma_page_fault(allocator, fault_addr, access)
    }

    fn munmap<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                 allocator : &mut A,
                                                                 addr : VirtAddr,
                                                                 len : usize)
                                                                 -> MmResult<PteChange> {
        if len == 0 {
            return Err(MmError::InvalidAddress);
        }
        let end = VirtAddr(addr.0
                               .checked_add(len)
                               .ok_or(MmError::InvalidAddress)?);
        if end.0 > crate::pagetable::USER_VA_LIMIT {
            return Err(MmError::InvalidAddress);
        }
        let page_start = addr.floor_page()
                             .start_addr();
        let page_end = end.ceil_page()
                          .start_addr();
        self.sync_shared_file_vmas(page_start, page_end)?;
        let changed = self.unmap_mmap_range(allocator, addr, end)?;
        self.remove_vmas(page_start, page_end)?;
        Ok(if changed {
            PteChange::Changed
        } else {
            PteChange::None
        })
    }

    fn munmap_external(&mut self, addr : VirtAddr, len : usize) -> MmResult<PteChange> {
        if len == 0 {
            return Err(MmError::InvalidAddress);
        }
        let end = VirtAddr(addr.0
                               .checked_add(len)
                               .ok_or(MmError::InvalidAddress)?);
        if end.0 > crate::pagetable::USER_VA_LIMIT {
            return Err(MmError::InvalidAddress);
        }
        let page_start = addr.floor_page()
                             .start_addr();
        let page_end = end.ceil_page()
                          .start_addr();
        if !self.vmas
                .covers_where(page_start,
                              page_end,
                              VmArea::is_external)
        {
            return Err(MmError::InvalidAddress);
        }
        let mut changed = false;
        let mut vpn = page_start.floor_page();
        let vpn_end = page_end.floor_page();
        while vpn.0 < vpn_end.0 {
            // 外部对象仍持有 PPN；这里故意丢弃返回值而不调用 frame_dealloc。
            changed |= self.unmap_page_to_ppn(vpn)?
                           .is_some();
            vpn = VirtPageNum(vpn.0 + 1);
        }
        self.remove_vmas(page_start, page_end)?;
        Ok(if changed {
            PteChange::Changed
        } else {
            PteChange::None
        })
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
        if end.0 > crate::pagetable::USER_VA_LIMIT {
            return Err(MmError::InvalidAddress);
        }
        let page_start = addr.floor_page()
                             .start_addr();
        let page_end = end.ceil_page()
                          .start_addr();
        if perm.executable() && self.device_vma_overlaps(page_start, page_end) {
            return Err(MmError::AccessViolation);
        }
        if !self.vmas
                .covers(page_start, page_end)
        {
            return Err(MmError::NotMapped);
        }
        let perm_u = perm | PagePerm::U;
        let mut vpn = addr.floor_page();
        let vpn_end = end.ceil_page();
        let mut ptes_changed = false;
        while vpn.0 < vpn_end.0 {
            let Some(old_pa) = self.translate_addr(vpn.start_addr())? else {
                let demand_paged = self.vmas
                                       .lookup(vpn.start_addr())
                                       .and_then(|index| self.vmas.get(index))
                                       .is_some_and(|vma| vma.demand_paged);
                if demand_paged {
                    vpn = VirtPageNum(vpn.0 + 1);
                    continue;
                }
                return Err(MmError::NotMapped);
            };
            let old_perm = self.leaf_page_perm(vpn)?
                               .ok_or(MmError::NotMapped)?;
            let vma = self.vmas
                          .lookup(vpn.start_addr())
                          .and_then(|index| self.vmas.get(index))
                          .ok_or(MmError::NotMapped)?;
            let private_page = !vma.is_device() && !vma.is_shared();
            if perm_u.writable() && private_page {
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
        self.protect_vmas(page_start, page_end, perm_u)?;
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
           self.range_overlaps_stack(old_addr, old_end)
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
                self.remove_vmas(new_address, end)?;
            }
        } else {
            let end = mmap_map_end(old_addr, new_size)?;
            if end.0 > crate::pagetable::USER_VA_LIMIT || self.range_overlaps_stack(old_addr, end) {
                return Err(MmError::InvalidAddress);
            }
        }
        let mut source_vma = self.vmas
                                 .duplicate_subrange(old_start, old_end)?
                                 .ok_or(MmError::Unsupported)?;
        if source_vma.is_shared() ||
           source_vma.is_device() ||
           matches!(source_vma.kind,
                    VmaKind::Heap | VmaKind::Stack)
        {
            return Err(MmError::Unsupported);
        }
        let perm = source_vma.perm;
        let requested_end = mmap_map_end(old_addr, new_size)?;
        let mut force_move = flags & MREMAP_FIXED == 0 &&
                             requested_end.0 > old_end.0 &&
                             self.vmas
                                 .overlaps(old_end, requested_end);
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
        if !keep_old {
            self.remove_vmas(old_start, old_end)?;
        }
        if result != old_start || keep_old {
            self.remove_vmas(result, result_end)?;
        }
        source_vma.start = result;
        source_vma.end = result_end;
        self.vmas
            .insert(source_vma)?;
        Ok(result)
    }
}

impl LoongArch64AddressSpace {
    /// 使统一表中所有 `demand_paged` VMA 全部驻留。
    pub fn prefault_all_current_user_ranges<A>(&mut self, allocator : &mut A) -> MmResult<()>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum> {
        let ranges : Vec<(VirtAddr, VirtAddr, PageFaultAccess)> =
            self.vmas
                .iter()
                .filter(|vma| vma.demand_paged)
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
                        return None;
                    };
                    Some((vma.start, vma.end, access))
                })
                .collect();
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
            if self.vmas
                   .lookup(page)
                   .is_none()
            {
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
        self.vmas
            .overlaps_where(addr, end, |vma| {
                vma.is_shared() || vma.kind == VmaKind::File || vma.is_device()
            })
    }

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
