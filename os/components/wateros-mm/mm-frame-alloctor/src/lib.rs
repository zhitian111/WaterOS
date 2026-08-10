//! 物理帧分配器聚合：按 feature 导出 API 与 **栈式**或 **dummy** 实现；为 `mm-impl` 页表与内核 bring-up 提供 `PhysPageNum` 粒度的帧。

#![no_std]

pub use api_v0::*;

use driver_api::{dma::DmaRegion, DriverResult};
use mm_api::addr::{PhysAddr, PhysPageNum, PAGE_SIZE};

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
            let frame = frame_alloc_result()?;
            unsafe {
                core::ptr::write_bytes((frame.0 * PAGE_SIZE) as *mut u8,
                                       0,
                                       PAGE_SIZE);
            }
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
        unsafe {
            core::slice::from_raw_parts((self.frame.0 * PAGE_SIZE) as *const u8,
                                        PAGE_SIZE)
        }
    }

    /// 独占借用整页可写字节。
    #[inline]
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut((self.frame.0 * PAGE_SIZE) as *mut u8,
                                            PAGE_SIZE)
        }
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

/// 独占拥有一段物理连续、按页对齐的 RAM。
///
/// 与 [`OwnedPhysPage`] 相同，字节借用依赖当前内核对可分配 RAM 的恒等映射；供设备
/// 使用的地址必须取自 [`Self::physical_address`]，不能从 slice 指针反推 DMA 地址。
/// 该对象不可复制，析构时原子归还整个连续区间。
pub struct OwnedPhysFrameSpan {
    span : FrameSpan<PhysPageNum>,
}

impl OwnedPhysFrameSpan {
    /// 分配并清零 `frame_count` 页；`alignment_frames` 必须为非零 2 的幂。
    pub fn alloc_zeroed(frame_count : usize, alignment_frames : usize) -> FrameAllocResult<Self> {
        #[cfg(feature = "impl-stack")]
        {
            let byte_len = frame_count.checked_mul(PAGE_SIZE)
                                      .ok_or(FrameAllocError::InvalidFrame)?;
            let span = frame_alloc_contiguous(frame_count, alignment_frames)?;
            unsafe {
                core::ptr::write_bytes(span.start()
                                           .start_addr()
                                           .0 as *mut u8,
                                       0,
                                       byte_len);
            }
            Ok(Self { span })
        }
        #[cfg(not(feature = "impl-stack"))]
        {
            let _ = (frame_count, alignment_frames);
            Err(FrameAllocError::Unsupported)
        }
    }

    /// 设备可见的连续物理首地址。
    #[inline]
    pub const fn physical_address(&self) -> PhysAddr {
        self.span
            .start()
            .start_addr()
    }

    /// 连续区间的字节长度。
    #[inline]
    pub const fn byte_len(&self) -> usize {
        self.span
            .frame_count() *
        PAGE_SIZE
    }

    /// 连续区间的页数。
    #[inline]
    pub const fn frame_count(&self) -> usize {
        self.span
            .frame_count()
    }

    /// Describe this owned span as a DMA region without transferring its
    /// allocation or assuming that virtual and physical addresses match.
    ///
    /// `virtual_address` must be the address of the mapping that the device
    /// driver will use.  The span remains the owner and must outlive every
    /// borrow of the returned region.  Platforms using the current identity
    /// mapping may pass `self.as_bytes().as_ptr() as usize`, but that is an
    /// explicit platform choice rather than a property of this API.
    pub fn dma_region(&self,
                      virtual_address : usize,
                      alignment : usize,
                      device_address_bits : u8)
                      -> DriverResult<DmaRegion> {
        DmaRegion::new(virtual_address,
                       self.physical_address()
                           .0 as u64,
                       self.byte_len(),
                       alignment,
                       device_address_bits)
    }

    /// 借用连续区间的只读恒等映射。
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self.physical_address()
                                            .0 as *const u8,
                                        self.byte_len())
        }
    }

    /// 独占借用连续区间的可写恒等映射。
    #[inline]
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.physical_address()
                                                .0
                                            as *mut u8,
                                            self.byte_len())
        }
    }
}

impl Drop for OwnedPhysFrameSpan {
    fn drop(&mut self) {
        #[cfg(feature = "impl-stack")]
        if let Err(error) = frame_dealloc_contiguous(self.span) {
            log::error!("[frame-allocator] owned span drop failed ppn={:#x} frames={}: {:?}",
                        self.span.start().0,
                        self.span
                            .frame_count(),
                        error);
        }
    }
}

/// 全局帧池只读统计（[`impl_stack::frame_mem_stats`] 或 dummy 零值）。
pub fn frame_mem_stats() -> FrameMemStats {
    #[cfg(feature = "impl-stack")]
    return impl_stack::frame_mem_stats();
    #[cfg(not(feature = "impl-stack"))]
    FrameMemStats::default()
}

#[cfg(feature = "impl-stack")]
pub use impl_stack::*;

#[cfg(feature = "impl-dummy")]
pub use impl_dummy::*;

/// 按当前 feature 运行帧分配器自测：`PhysPageNum` 为半开区间 `[start, end)`，
/// 与 `init_frame_allocator` 约定一致；dummy 实现仅打日志。
pub fn test_with_range(start_ppn : mm_api::addr::PhysPageNum, end_ppn : mm_api::addr::PhysPageNum) {
    log::trace!("[frame-alloctor] test begin");
    #[cfg(feature = "impl-stack")]
    impl_stack::test_with_range(start_ppn, end_ppn);
    #[cfg(feature = "impl-dummy")]
    {
        let _ = (start_ppn, end_ppn);
        log::info!("[frame-alloctor] dummy impl: no test");
    }
    log::trace!("[frame-alloctor] test end");
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::ManuallyDrop;

    #[test]
    fn dma_region_keeps_owned_span_physical_address_and_explicit_virtual_address() {
        let span =
            ManuallyDrop::new(OwnedPhysFrameSpan { span : FrameSpan::new(PhysPageNum(2), 2) });
        let region = span.dma_region(0x20_000, PAGE_SIZE, 32)
                         .unwrap();
        assert_eq!(region.virtual_address(), 0x20_000);
        assert_eq!(region.physical_address(),
                   2 * PAGE_SIZE as u64);
        assert_eq!(region.length(), 2 * PAGE_SIZE);
        assert_eq!(region.alignment(), PAGE_SIZE);
        assert_eq!(span.dma_region(0x20_000, PAGE_SIZE, 12),
                   Err(driver_api::DriverError::InvalidParam));
    }
}
