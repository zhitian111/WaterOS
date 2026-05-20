//! 用户堆 `brk` 与匿名 `mmap`/`munmap`/`mprotect` 的 Sv39 实现。
//!
//! # 缺页与 trap
//!
//! 与 [`crate::pagetable`] 一致：映射 **饥渴（eager）** 建立；未映射用户 VA 由硬件 page fault
//! 进入 trap，本阶段不提供 demand paging。

use api_v0::addr::{PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::brk::{BrkRegion, HeapBrk};
use api_v0::error::{MmError, MmResult};
use api_v0::flags::MapFlags;
use api_v0::frame_allocator::PhysicalFrameAllocator;
use api_v0::mmap::{MmapKind, MmapOps, MmapRequest};
use api_v0::perm::PagePerm;

use crate::pagetable::Sv39AddressSpace;

#[inline]
fn fence_user_ptes() {
    platform::arch::paging::flush_address_space_translations();
}

impl HeapBrk for Sv39AddressSpace {
    fn brk_region(&self) -> BrkRegion {
        BrkRegion {
            start: self.user_brk_start,
            current_end: self.user_brk_current_end,
            max: self.user_brk_max,
        }
    }

    fn brk<A: PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator: &mut A,
        new_end: VirtAddr,
    ) -> MmResult<VirtAddr> {
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
            let end_vpn_excl = VirtAddr(new_end.0).ceil_page().0;
            let mut vpn_i = VirtAddr(r.current_end.0).floor_page().0;
            while vpn_i < end_vpn_excl {
                let vpn = VirtPageNum(vpn_i);
                if self.translate_addr(vpn.start_addr())?.is_none() {
                    self.map_page_with_alloc(allocator, vpn, Self::brk_perm())?;
                }
                vpn_i += 1;
            }
        }
        self.user_brk_current_end = new_end;
        fence_user_ptes();
        Ok(new_end)
    }
}

impl MmapOps for Sv39AddressSpace {
    fn mmap<A: PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator: &mut A,
        req: MmapRequest,
    ) -> MmResult<VirtAddr> {
        if !matches!(req.kind, MmapKind::Anonymous) {
            return Err(MmError::Unsupported);
        }
        if !req.flags.contains(MapFlags::ANONYMOUS) || !req.flags.contains(MapFlags::PRIVATE) {
            return Err(MmError::Unsupported);
        }
        if req.len == 0 {
            return Err(MmError::InvalidAddress);
        }
        let n_pages = req
            .len
            .checked_add(PAGE_SIZE - 1)
            .ok_or(MmError::InvalidAddress)?
            / PAGE_SIZE;
        let map_end = |base: VirtAddr| -> MmResult<VirtAddr> {
            Ok(VirtAddr(
                base.0
                    .checked_add(n_pages * PAGE_SIZE)
                    .ok_or(MmError::InvalidAddress)?,
            ))
        };
        let base = match req.addr_hint {
            Some(hint) if req.flags.contains(MapFlags::FIXED) => hint,
            Some(_) => return Err(MmError::Unsupported),
            None => self.mmap_anon_cursor,
        };
        let end = map_end(base)?;
        let perm = req.prot | PagePerm::U;
        if perm == PagePerm::U {
            return Err(MmError::Unsupported);
        }
        self.map_range_with_alloc(allocator, base, end, perm)?;
        if req.addr_hint.is_none() {
            self.mmap_anon_cursor = end;
        }
        fence_user_ptes();
        Ok(base)
    }

    fn munmap<A: PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator: &mut A,
        addr: VirtAddr,
        len: usize,
    ) -> MmResult<()> {
        if len == 0 {
            return Err(MmError::InvalidAddress);
        }
        let end = VirtAddr(
            addr.0
                .checked_add(len)
                .ok_or(MmError::InvalidAddress)?,
        );
        self.unmap_range_with_alloc(allocator, addr, end)?;
        fence_user_ptes();
        Ok(())
    }

    fn mprotect(&mut self, addr: VirtAddr, len: usize, perm: PagePerm) -> MmResult<()> {
        if len == 0 {
            return Ok(());
        }
        if perm == PagePerm::empty() {
            return Err(MmError::Unsupported);
        }
        let end = VirtAddr(addr.0.checked_add(len).ok_or(MmError::InvalidAddress)?);
        let mut vpn = addr.floor_page();
        let vpn_end = end.ceil_page();
        let perm_u = perm | PagePerm::U;
        while vpn.0 < vpn_end.0 {
            if self.translate_addr(vpn.start_addr())?.is_none() {
                return Err(MmError::NotMapped);
            }
            self.protect_page(vpn, perm_u)?;
            vpn = VirtPageNum(vpn.0 + 1);
        }
        fence_user_ptes();
        Ok(())
    }
}
