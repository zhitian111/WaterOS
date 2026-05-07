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

/// 物理帧分配器：为页表映射提供“分配/回收最小粒度”的能力。
///
/// 该 trait 本身只关心“帧标识（FrameId）”，不直接绑定特定页表格式（Sv39等属于 mm-impl 的职责）。
pub trait PhysicalFrameAllocator {
    /// 物理帧标识类型（通常可与 PPN 对齐）
    type FrameId: Copy + Eq;

    /// 分配一个物理帧。
    fn alloc_frame(&mut self) -> FrameAllocResult<Self::FrameId>;

    /// 释放一个物理帧。
    fn dealloc_frame(&mut self, frame: Self::FrameId) -> FrameAllocResult<()>;
}
