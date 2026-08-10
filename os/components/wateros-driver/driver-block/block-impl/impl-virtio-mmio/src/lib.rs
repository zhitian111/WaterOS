//! VirtIO 块设备（MMIO 传输）实现，供平台驱动在枚举到 `virtio,mmio` + block 设备后实例化。
//!
//! **DMA / HAL**：`virtio-drivers` 的队列与内部缓冲通过 [`Hal::dma_alloc`] /
//! [`Hal::dma_dealloc`] 向本机要 **物理连续、页对齐、已清零** 的内存。此前使用固定 bump
//! 物理地址且 `dma_dealloc` 为空实现，易与内核其它内存及 VirtIO 传输重叠；现改为使用
//! 已初始化的全局 **帧分配器**（`wateros-mm-frame-alloctor`），与 Sv39 bring-up 一致。
//! 恒等映射下 `paddr == vaddr`（`usize`/`PhysAddr` 视图一致）。

#![no_std]
extern crate alloc;

use core::ptr::NonNull;

use api_v0::{BlockDevice, DriverError, DriverResult, Lba};
use driver_api::MmioRegion;
use driver_common::virtio_dma;
use virtio_drivers::device::blk::VirtIOBlk;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

const _ : () = assert!(PAGE_SIZE == mm_api::addr::PAGE_SIZE);
const IOZONE_PROBE_MIN_WRITE_BYTES : usize = 4096;

/// 将内核帧分配器接到 `virtio-drivers` 的 [`Hal`]：恒等映射下返回的 `PhysAddr` 与可写虚拟指针相同。
struct VirtioMmioHal;

unsafe impl Hal for VirtioMmioHal {
    /// 按页数向帧池要连续物理页；失败时释放已拿页并返回空指针对，由上层映射为 [`DriverError`]。
    fn dma_alloc(pages : usize, _direction : BufferDirection) -> (PhysAddr, NonNull<u8>) {
        virtio_dma::alloc(pages).map(|(address, pointer)| (address as PhysAddr, pointer))
                                .unwrap_or((0, NonNull::dangling()))
    }

    unsafe fn dma_dealloc(paddr : PhysAddr, vaddr : NonNull<u8>, pages : usize) -> i32 {
        unsafe { virtio_dma::dealloc(paddr as u64, vaddr, pages) }
    }

    unsafe fn mmio_phys_to_virt(paddr : PhysAddr, _size : usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("mmio_phys_to_virt: null")
    }

    unsafe fn share(buffer : NonNull<[u8]>, _direction : BufferDirection) -> PhysAddr {
        unsafe { virtio_dma::share_identity(buffer) as PhysAddr }
    }

    unsafe fn unshare(_paddr : PhysAddr, _buffer : NonNull<[u8]>, _direction : BufferDirection) {}
}

/// VirtIO-MMIO 上的块设备（`virtio-blk`）。
pub struct VirtioBlkDevice {
    /// `virtio-drivers` 侧已握手的传输与队列状态。
    inner : VirtIOBlk<VirtioMmioHal, MmioTransport<'static>>,
}

impl VirtioBlkDevice {
    /// 在给定 MMIO 窗口内探测并初始化 `virtio-blk`；头指针或传输握手失败时映射为 [`DriverError`]。
    ///
    /// **须在** `init_frame_allocator`（或等价全局帧池初始化）**之后**调用。
    pub fn from_mmio(mmio : MmioRegion) -> DriverResult<Self> {
        let header = NonNull::new(mmio.base as *mut VirtIOHeader).ok_or(DriverError::InvalidDtb)?;
        let transport =
            unsafe { MmioTransport::new(header, mmio.size) }.map_err(|_| DriverError::Unsupported)?;
        let inner =
            VirtIOBlk::<VirtioMmioHal, MmioTransport>::new(transport).map_err(|_| {
                                                                         DriverError::Unsupported
                                                                     })?;
        Ok(Self { inner })
    }
}

impl BlockDevice for VirtioBlkDevice {
    /// Expose the VirtIO capacity in 512-byte sectors so partition scanners can
    /// validate primary/backup metadata against the actual device boundary.
    fn total_blocks(&self) -> Option<u64> {
        Some(self.inner
                 .capacity())
    }

    /// 以 LBA 为单位读入 `buf`；长度须为块大小的整数倍，否则由 VirtIO 层返回错误。
    fn read_blocks(&mut self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
        self.inner
            .read_blocks(start_block.0 as usize, buf)
            .map_err(|_| DriverError::IoError)
    }

    /// 将 `buf` 写回磁盘；语义与 [`read_blocks`] 对称。
    fn write_blocks(&mut self, start_block : Lba, buf : &[u8]) -> DriverResult<()> {
        let probe = buf.len() >= IOZONE_PROBE_MIN_WRITE_BYTES;
        if probe {
            logging::trace!("[virtio-blk-write] begin lba={} bytes={}",
                            start_block.0,
                            buf.len());
        }
        let result = self.inner
                         .write_blocks(start_block.0 as usize, buf)
                         .map_err(|_| DriverError::IoError);
        if probe {
            match &result {
                Ok(()) => {
                    logging::trace!("[virtio-blk-write] end lba={} bytes={} ret=ok",
                                    start_block.0,
                                    buf.len());
                }
                Err(err) => {
                    logging::trace!("[virtio-blk-write] end lba={} bytes={} err={:?}",
                                    start_block.0,
                                    buf.len(),
                                    err);
                }
            }
        }
        result
    }
}
