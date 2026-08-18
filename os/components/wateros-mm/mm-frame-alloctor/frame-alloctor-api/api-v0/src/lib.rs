//! 物理帧分配 API v0：分配粒度为 **一帧 = 一页物理存储**（WaterOS 中与 mm-api 的 `PhysPageNum` 对齐，通常为 4 KiB 物理页号）。

#![no_std]

use core::result::Result;

/// 帧分配错误（尽量保持语义简洁，便于 mm-api 做统一错误映射）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAllocError {
    /// 没有可用帧；调用方可回收可替换资源后重试，但不得在持锁状态无界忙等。
    OutOfMemory,
    /// 释放了不合法、保留或未分配的帧；这通常意味着映射/引用计数生命周期错误。
    InvalidFrame,
    /// 当前操作不支持，例如 dummy 分配器不提供真实帧。
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
    /// 一帧的页大小（字节）；统计转换以此为准，不能假设所有未来平台均为 4 KiB。
    pub page_bytes: usize,
}

impl FrameMemStats {
    #[inline]
    /// 以饱和乘法计算池总容量，避免大内存配置在统计路径回绕。
    pub const fn total_bytes(self) -> u64 {
        (self.total_frames as u64).saturating_mul(self.page_bytes as u64)
    }

    #[inline]
    /// 以饱和乘法计算当前空闲容量；这是瞬时快照，不保证随后分配仍成功。
    pub const fn free_bytes(self) -> u64 {
        (self.free_frames as u64).saturating_mul(self.page_bytes as u64)
    }

    #[inline]
    /// 总容量减空闲容量；若实现给出不一致统计，饱和减法仍避免出现巨大的回绕值。
    pub const fn used_bytes(self) -> u64 {
        self.total_bytes().saturating_sub(self.free_bytes())
    }
}

/// 物理帧分配器：为页表映射提供“分配/回收最小粒度”的能力。
///
/// 该 trait 本身只关心“帧标识（FrameId）”，不直接绑定特定页表格式（Sv39等属于 mm-impl 的职责）。
pub trait PhysicalFrameAllocator {
    /// 物理帧标识类型（通常可与 PPN 对齐）；只可复制其数值，不能据此复制一份所有权。
    type FrameId: Copy + Eq;

    /// 分配一帧；耗尽时返回 [`FrameAllocError::OutOfMemory`]。
    /// 成功时调用方独占一份引用，须在不再被页表、缓存或 DMA 使用后恰好归还一次。
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
    /// 若实现支持引用计数，这会释放一份引用而不一定立即把帧放回空闲池。
    fn dealloc_frame(&mut self, frame: Self::FrameId) -> FrameAllocResult<()>;
}
