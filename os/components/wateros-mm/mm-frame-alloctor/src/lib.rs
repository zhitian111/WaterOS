//! 物理帧分配器聚合：按 feature 导出 API 与 **栈式**或 **dummy** 实现；为 `mm-impl` 页表与内核 bring-up 提供 `PhysPageNum` 粒度的帧。

#![no_std]

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[mm/frame-allocator] self_test begin");
    impl_stack::self_test();
    log::info!("[mm/frame-allocator] self_test complete");
}

pub use api_v0::*;

use mm_api::addr::{PhysPageNum, PAGE_SIZE};

/// 独占拥有一页、可由内核通过 RAM 恒等映射访问的物理帧。
///
/// 当前 RISC-V64 与 LoongArch64 内核页表都会恒等映射完整可分配 RAM。该类型将这一
/// 组装契约和“恰好回收一次”的责任封装在 frame allocator 层；使用方不能取得可复制
/// 的所有权句柄，也不能让借出的 slice 越过 `self` 生命周期。
pub struct OwnedPhysPage {
    frame : PhysPageNum,
}

impl OwnedPhysPage {
    /// 分配并清零一页。帧池未初始化仍属于启动顺序错误；帧耗尽则正常返回错误。
    pub fn alloc_zeroed() -> FrameAllocResult<Self> {
        #[cfg(feature = "impl-stack")]
        {
            let frame = frame_alloc_zeroed_result()?;
            Ok(Self { frame })
        }
        #[cfg(not(feature = "impl-stack"))]
        Err(FrameAllocError::Unsupported)
    }

    /// 物理页号，仅供统计和诊断；不转移所有权。
    #[inline]
    pub const fn frame_id(&self) -> PhysPageNum { self.frame }

    /// 借用整页只读字节。
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts((self.frame.0 * PAGE_SIZE) as *const u8, PAGE_SIZE) }
    }

    /// 独占借用整页可写字节。
    #[inline]
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut((self.frame.0 * PAGE_SIZE) as *mut u8, PAGE_SIZE) }
    }
}

impl Drop for OwnedPhysPage {
    fn drop(&mut self) {
        #[cfg(feature = "impl-stack")]
        if let Err(error) = frame_dealloc_result(self.frame) {
            log::error!("[frame-allocator] owned page drop failed ppn={:#x}: {:?}",
                        self.frame.0,
                        error);
        }
    }
}

/// 全局帧池只读统计。
pub fn frame_mem_stats() -> FrameMemStats {
    #[cfg(feature = "impl-stack")]
    return impl_stack::frame_mem_stats();
    #[cfg(not(feature = "impl-stack"))]
    FrameMemStats::default()
}

#[cfg(feature = "impl-stack")]
pub use impl_stack::*;


/// 按当前 feature 运行帧分配器自测：`PhysPageNum` 为半开区间 `[start, end)`，
/// 与 `init_frame_allocator` 约定一致。
pub fn test_with_range(start_ppn : mm_api::addr::PhysPageNum,
                       end_ppn : mm_api::addr::PhysPageNum) {
    log::trace!("[frame-alloctor] test begin");
    #[cfg(feature = "impl-stack")]
    impl_stack::test_with_range(start_ppn, end_ppn);
    log::trace!("[frame-alloctor] test end");
}
