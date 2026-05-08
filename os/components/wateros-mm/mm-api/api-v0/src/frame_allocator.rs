//! Re-export 物理帧分配器 trait 与错误类型（定义在 `frame-alloctor-api`，实现由 `frame-alloctor` 选择）。
//!
//! [`PhysicalFrameAllocator::FrameId`] 在 Sv39 路径上与 [`crate::addr::PhysPageNum`] 对齐，便于页表项写入 PPN。
//! mm-api 自身不链接具体分配器实现；根内核或 `wateros-mm` 通过依赖选择 `impl-stack` / dummy。

pub use frame_alloctor_api_v0::{
    FrameAllocError, FrameAllocResult, PhysicalFrameAllocator,
};

