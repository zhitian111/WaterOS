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
    /// 堆起始虚拟地址，通常等于 ELF 数据段之后的初始 break；该边界不可由 `brk` 缩小越过。
    pub start: VirtAddr,
    /// 当前堆尾（brk）；可非页对齐，实现扩展时应按页向上取整映射。
    pub current_end: VirtAddr,
    /// 堆允许增长的上限（半开区间端点）；与现有 VMA 冲突或超过此值的请求必须失败或保持旧值。
    pub max: VirtAddr,
}

/// 堆增长接口契约（glibc 需要）。
///
/// brk 的语义是调整堆的结束边界；当 `new_end` 超过 `current_end`
/// 时需要为新增 **4 KiB** 虚拟页分配并映射（与 [`crate::addr::PAGE_SIZE`] 对齐的页边界）。
pub trait HeapBrk: AddressSpaceOps {
    /// 返回当前堆布局（syscall `brk` 查询路径使用）。
    fn brk_region(&self) -> BrkRegion;

    /// 将堆尾调整到 `new_end`；跨越新页时须分配并映射整页。
    ///
    /// 成功返回实际生效的堆尾（可能受 `max` 或实现策略限制而与请求不同，具体由实现约定）。
    /// 缩小时必须只回收不再覆盖新半开区间的整页，保留末页可避免相邻仍有效字节被误解除映射。
    fn brk<A: PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator: &mut A,
        new_end: VirtAddr,
    ) -> MmResult<VirtAddr>;

    /// 堆扩展映射的默认页权限：R/W/U（与常见 glibc 期望一致）。
    #[inline]
    fn brk_perm() -> PagePerm { PagePerm::R | PagePerm::W | PagePerm::U }
}
