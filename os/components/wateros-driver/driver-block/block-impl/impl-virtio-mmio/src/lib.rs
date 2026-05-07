//! VirtIO 块设备（MMIO 传输）实现，供平台驱动在枚举到 `virtio,mmio` + block 设备后实例化。
//!
//! DMA 与物理地址策略与 QEMU bring-up 一致：恒等映射下的简化 bump 分配，仅适用于早期自检场景。

#![no_std]
extern crate alloc;

use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{BlockDevice, DriverError, DriverResult, Lba};
use driver_api::MmioRegion;
use virtio_drivers::device::blk::VirtIOBlk;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::{BufferDirection, Hal, PhysAddr};

// 与 QEMU virt bring-up 一致的 DMA 物理游标起点；非通用分配器，仅早期 virtio 使用。
static DMA_CURSOR: AtomicUsize = AtomicUsize::new(0x8100_0000);

struct VirtioMmioHal;

unsafe impl Hal for VirtioMmioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let bytes = pages * 4096;
        let paddr = DMA_CURSOR.fetch_add(bytes, Ordering::Relaxed) as PhysAddr;
        (paddr, NonNull::new(paddr as *mut u8).unwrap())
    }

    unsafe fn dma_dealloc(_paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 { 0 }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).unwrap()
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize as PhysAddr
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}

/// VirtIO-MMIO 上的块设备（`virtio-blk`）。
pub struct VirtioBlkDevice {
    inner: VirtIOBlk<VirtioMmioHal, MmioTransport<'static>>,
}

impl VirtioBlkDevice {
    /// 在给定 MMIO 窗口内探测并初始化 `virtio-blk`；头指针或传输握手失败时映射为 [`DriverError`]。
    pub fn from_mmio(mmio: MmioRegion) -> DriverResult<Self> {
        let header = NonNull::new(mmio.base as *mut VirtIOHeader).ok_or(DriverError::InvalidDtb)?;
        let transport =
            unsafe { MmioTransport::new(header, mmio.size) }.map_err(|_| DriverError::Unsupported)?;
        let inner = VirtIOBlk::<VirtioMmioHal, MmioTransport>::new(transport)
            .map_err(|_| DriverError::Unsupported)?;
        Ok(Self { inner })
    }
}

impl BlockDevice for VirtioBlkDevice {
    fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
        self.inner
            .read_blocks(start_block.0 as usize, buf)
            .map_err(|_| DriverError::IoError)
    }

    fn write_blocks(&mut self, start_block: Lba, buf: &[u8]) -> DriverResult<()> {
        self.inner
            .write_blocks(start_block.0 as usize, buf)
            .map_err(|_| DriverError::IoError)
    }
}
