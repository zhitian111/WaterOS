//! Re-export 物理帧分配器 trait 与错误类型（定义在 `frame-alloctor-api`，实现由 `frame-alloctor` 选择）。
//!
//! [`PhysicalFrameAllocator::FrameId`] 在 Sv39 路径上与 [`crate::addr::PhysPageNum`] 对齐，便于页表项写入 PPN。

pub use frame_alloctor_api_v0::{
    FrameAllocError, FrameAllocResult, PhysicalFrameAllocator,
};

