//! 地址空间操作契约：以 **4 KiB 虚拟页** 为粒度（见 [`crate::addr::PAGE_SIZE`]），`satp_value` 供安装页表。
//! 切换 `satp` 与 TLB 刷新由 arch/运行时完成；本 trait 本身不在 trap 内执行，也不隐含额外硬件副作用。

use crate::addr::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
use crate::error::MmResult;
use crate::frame_allocator::PhysicalFrameAllocator;
use crate::perm::PagePerm;

/// 地址空间标识（ASID 等）；API 预留字段，当前 bring-up 可不使用或与 `satp` ASID 域对齐。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AddressSpaceId(pub u32);

/// 地址空间（地址映射）操作契约。
///
/// - 低级方法直接操作页到物理页（ppn）的映射；
/// - 默认方法提供“基于帧分配器分配/回收”的便利封装；
/// - `satp_value()` 用于在 arch-impl 中安装页表并刷新 TLB。
pub trait AddressSpaceOps {
    /// 安装页表所需的 `satp` 值（由 mm-impl/sv39 决定编码）。
    fn satp_value(&self) -> usize;

    /// 将 `vpn -> ppn` 映射到页表，并应用页权限 `perm`。
    fn map_page_to_ppn(
        &mut self,
        vpn: VirtPageNum,
        ppn: PhysPageNum,
        perm: PagePerm,
    ) -> MmResult<()>;

    /// 解除 `vpn` 对应映射。
    ///
    /// 返回解除前的 `ppn`（若 vpn 未映射则返回 `Ok(None)`）。
    fn unmap_page_to_ppn(&mut self, vpn: VirtPageNum) -> MmResult<Option<PhysPageNum>>;

    /// 更新 `vpn` 的权限（不改变映射）。
    fn protect_page(&mut self, vpn: VirtPageNum, perm: PagePerm) -> MmResult<()>;

    /// 翻译用户虚拟地址到物理地址。
    fn translate_addr(&self, va: VirtAddr) -> MmResult<Option<PhysAddr>>;

    /// 为 `vpn` 分配新帧并映射（匿名映射/缺页填充常用）。
    #[inline]
    fn map_page_with_alloc<A>(
        &mut self,
        allocator: &mut A,
        vpn: VirtPageNum,
        perm: PagePerm,
    ) -> MmResult<()>
    where
        A: PhysicalFrameAllocator<FrameId = PhysPageNum>,
    {
        let ppn = allocator.alloc_frame()?;
        self.map_page_to_ppn(vpn, ppn, perm)
    }

    /// 解除 `vpn` 映射并回收帧（若未映射则不执行任何操作）。
    #[inline]
    fn unmap_page_with_alloc<A>(
        &mut self,
        allocator: &mut A,
        vpn: VirtPageNum,
    ) -> MmResult<()>
    where
        A: PhysicalFrameAllocator<FrameId = PhysPageNum>,
    {
        if let Some(ppn) = self.unmap_page_to_ppn(vpn)? {
            allocator.dealloc_frame(ppn)?;
        }
        Ok(())
    }

    /// 将 `[start, end)` 覆盖的虚拟页全部映射到新分配的帧。
    #[inline]
    fn map_range_with_alloc<A>(
        &mut self,
        allocator: &mut A,
        start: VirtAddr,
        end: VirtAddr,
        perm: PagePerm,
    ) -> MmResult<()>
    where
        A: PhysicalFrameAllocator<FrameId = PhysPageNum>,
    {
        if start.0 >= end.0 {
            return Ok(());
        }
        let mut vpn = start.floor_page();
        let vpn_end = end.ceil_page();
        while vpn.0 < vpn_end.0 {
            self.map_page_with_alloc(allocator, vpn, perm)?;
            vpn = VirtPageNum(vpn.0 + 1);
        }
        Ok(())
    }

    /// 解除 `[start, end)` 覆盖的虚拟页映射，并回收对应帧。
    #[inline]
    fn unmap_range_with_alloc<A>(
        &mut self,
        allocator: &mut A,
        start: VirtAddr,
        end: VirtAddr,
    ) -> MmResult<()>
    where
        A: PhysicalFrameAllocator<FrameId = PhysPageNum>,
    {
        if start.0 >= end.0 {
            return Ok(());
        }
        let mut vpn = start.floor_page();
        let vpn_end = end.ceil_page();
        while vpn.0 < vpn_end.0 {
            self.unmap_page_with_alloc(allocator, vpn)?;
            vpn = VirtPageNum(vpn.0 + 1);
        }
        Ok(())
    }
}

