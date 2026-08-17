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

/// 物理帧池只读统计（供 `/proc/meminfo` 等）；字节数基于 4 KiB 页。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameMemStats {
    /// 池内总帧数。
    pub total_frames: usize,
    /// 当前空闲帧数。
    pub free_frames: usize,
    /// 页大小（字节）。
    pub page_bytes: usize,
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
        self.total_bytes().saturating_sub(self.free_bytes())
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

    /// 尝试从实现维护的预清零池分配一帧。
    ///
    /// `Ok(Some(frame))` 保证该帧的全部字节均为零，并把一份正常的所有权转移给
    /// 调用者；`Ok(None)` 表示该 allocator 不提供此可选能力，调用者应退回
    /// [`Self::alloc_frame`] 后自行清零。默认实现保持现有 allocator 的行为。
    fn try_alloc_zeroed_frame(&mut self) -> FrameAllocResult<Option<Self::FrameId>> {
        Ok(None)
    }

    /// 释放先前由 [`Self::alloc_frame`] 返回的帧；重复释放等行为由具体实现定义（栈实现见其实现侧注释）。
    fn dealloc_frame(&mut self, frame: Self::FrameId) -> FrameAllocResult<()>;
}
