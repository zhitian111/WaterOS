//! 堆（`brk`）契约：边界为 [`VirtAddr`]；增长跨越新页时需分配并映射 **整页**（4 KiB，与 [`crate::addr::PAGE_SIZE`] 一致）。

use crate::addr::VirtAddr;
use crate::address_space::AddressSpaceOps;
use crate::error::MmResult;
use crate::frame_allocator::PhysicalFrameAllocator;
use crate::addr::PhysPageNum;
use crate::perm::PagePerm;

/// brk 堆区间信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrkRegion {
    /// 堆起始虚拟地址。
    pub start: VirtAddr,
    /// 当前堆尾（brk）；可非页对齐，实现扩展时应按页向上取整映射。
    pub current_end: VirtAddr,
    /// 堆允许增长的上限。
    pub max: VirtAddr,
}

/// 堆增长接口契约（glibc 需要）。
///
/// brk 的语义是调整堆的结束边界；当 `new_end` 超过 `current_end`
/// 时需要为新增 **4 KiB** 虚拟页分配并映射（与 [`crate::addr::PAGE_SIZE`] 对齐的页边界）。
pub trait HeapBrk: AddressSpaceOps {
    /// 获取堆区间信息（用于 syscall 语义）。
    fn brk_region(&self) -> BrkRegion;

    /// 将堆边界调整到 `new_end`，返回最终生效的边界。
    fn brk<A: PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator: &mut A,
        new_end: VirtAddr,
    ) -> MmResult<VirtAddr>;

    /// brk 默认默认页权限：R/W/U（Linux glibc 通常需要用户可写）。
    #[inline]
    fn brk_perm() -> PagePerm { PagePerm::R | PagePerm::W | PagePerm::U }
}

