//! 物理帧分配 API v0：分配粒度为 **一帧 = 一页物理存储**（WaterOS 中与 mm-api 的 `PhysPageNum` 对齐，通常为 4 KiB 物理页号）。

#![no_std]

use core::result::Result;

/// 帧分配错误（尽量保持语义简洁，便于 mm-api 做统一错误映射）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAllocError {
    /// 没有可用帧
    OutOfMemory,
    /// 释放了不合法/未分配的帧（具体语义由实现决定）
    InvalidFrame,
    /// 当前操作不支持
    Unsupported,
}

pub type FrameAllocResult<T> = Result<T, FrameAllocError>;

/// 由分配器返回的一段连续 frame id；区间为 `[start, start + frame_count)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSpan<FrameId> {
    start : FrameId,
    frame_count : usize,
}

impl<FrameId : Copy> FrameSpan<FrameId> {
    /// 构造区间描述；合法性与所有权由具体分配器在分配/释放时校验。
    pub const fn new(start : FrameId, frame_count : usize) -> Self { Self { start, frame_count } }
    /// 连续区间的第一个 frame id。
    pub const fn start(self) -> FrameId { self.start }
    /// 连续区间包含的 frame 数量。
    pub const fn frame_count(self) -> usize { self.frame_count }
}

/// 物理帧池只读统计（供 `/proc/meminfo` 等）；字节数基于 4 KiB 页。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameMemStats {
    /// 池内总帧数。
    pub total_frames : usize,
    /// 当前空闲帧数。
    pub free_frames : usize,
    /// 页大小（字节）。
    pub page_bytes : usize,
}

impl FrameMemStats {
    #[inline]
    pub const fn total_bytes(self) -> u64 {
        (self.total_frames as u64).saturating_mul(self.page_bytes as u64)
    }

    #[inline]
    pub const fn free_bytes(self) -> u64 {
        (self.free_frames as u64).saturating_mul(self.page_bytes as u64)
    }

    #[inline]
    pub const fn used_bytes(self) -> u64 {
        self.total_bytes()
            .saturating_sub(self.free_bytes())
    }
}

/// 物理帧分配器：为页表映射提供“分配/回收最小粒度”的能力。
///
/// 该 trait 本身只关心“帧标识（FrameId）”，不直接绑定特定页表格式（Sv39等属于 mm-impl 的职责）。
pub trait PhysicalFrameAllocator {
    /// 物理帧标识类型（通常可与 PPN 对齐）
    type FrameId: Copy + Eq;

    /// 分配一帧；耗尽时返回 [`FrameAllocError::OutOfMemory`]。
    fn alloc_frame(&mut self) -> FrameAllocResult<Self::FrameId>;

    /// 释放先前由 [`Self::alloc_frame`] 返回的帧；重复释放等行为由具体实现定义（栈实现见其实现侧注释）。
    fn dealloc_frame(&mut self, frame : Self::FrameId) -> FrameAllocResult<()>;

    /// 原子分配连续区间；alignment 以 frame 为单位且由实现定义合法取值。
    fn alloc_contiguous(&mut self,
                        _frame_count : usize,
                        _alignment_frames : usize)
                        -> FrameAllocResult<FrameSpan<Self::FrameId>> {
        Err(FrameAllocError::Unsupported)
    }

    /// 原子释放先前分配的完整连续区间。
    fn dealloc_contiguous(&mut self, _span : FrameSpan<Self::FrameId>) -> FrameAllocResult<()> {
        Err(FrameAllocError::Unsupported)
    }
}
