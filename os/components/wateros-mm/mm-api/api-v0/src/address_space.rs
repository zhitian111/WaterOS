//! 地址空间操作契约：以 **4 KiB 虚拟页** 为粒度（见 [`crate::addr::PAGE_SIZE`]）。
//!
//! `satp_value` 是历史命名，实际含义是当前架构可安装的地址空间 token：
//! RISC-V 为 `satp` 编码，LoongArch64 为 PGDL 与 ASID 的组合编码。安装 token
//! 与 TLB 刷新由 arch/platform 层完成；本 trait 本身不隐含额外硬件副作用。

use crate::addr::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
use crate::error::{MmError, MmResult};
use crate::frame_allocator::PhysicalFrameAllocator;
use crate::perm::PagePerm;

/// 地址空间标识（ASID 等）；具体分配与复用规则由架构 MM 实现负责。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AddressSpaceId(pub u32);

/// 地址空间（地址映射）操作契约。
///
/// - 低级方法直接操作页到物理页（ppn）的映射；
/// - 默认方法提供“基于帧分配器分配/回收”的便利封装；
/// - `satp_value()` 用于在 arch-impl 中安装页表并刷新 TLB。
pub trait AddressSpaceOps {
    /// 安装页表所需的地址空间 token（历史命名为 `satp`）。
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

    /// 若 `vpn` 已映射为叶子页，返回当前语义层权限；否则 `Ok(None)`。
    ///
    /// 典型用途：ELF 多个 `PT_LOAD` 共享同一虚拟页时合并权限。默认实现恒为 `None`（不合并），
    /// 仅具备叶子页语义的 mm-impl（如 Sv39）需要覆盖。
    fn leaf_page_perm(&self, _vpn: VirtPageNum) -> MmResult<Option<PagePerm>> {
        Ok(None)
    }

    /// 创建一个独立的地址空间副本：所有带 `U`（用户态可访问）权限的叶子页逐帧复制，
    /// 不带 `U` 的内核恒等映射等页保持共享。
    ///
    /// 默认实现返回 [`MmError::Unsupported`]（dummy / 不支持独立地址空间的情形）。
    fn fork(&self) -> MmResult<Self>
    where
        Self: Sized,
    {
        Err(MmError::Unsupported)
    }

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

    /// 解除 `vpn` 映射并回收帧；返回是否实际移除了叶 PTE。
    #[inline]
    fn unmap_page_with_alloc<A>(
        &mut self,
        allocator: &mut A,
        vpn: VirtPageNum,
    ) -> MmResult<bool>
    where
        A: PhysicalFrameAllocator<FrameId = PhysPageNum>,
    {
        if let Some(ppn) = self.unmap_page_to_ppn(vpn)? {
            allocator.dealloc_frame(ppn)?;
            return Ok(true);
        }
        Ok(false)
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
