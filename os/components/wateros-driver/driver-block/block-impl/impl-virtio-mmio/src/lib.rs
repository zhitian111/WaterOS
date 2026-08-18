//! VirtIO 块设备（MMIO 传输）实现，供平台驱动在枚举到 `virtio,mmio` + block 设备后实例化。
//!
//! **DMA / HAL**：统一 HAL 从 linker 保留的固定 DMA pool 申请物理连续、页对齐、已清零的
//! 队列内存；普通 I/O buffer 通过 HAL 的 staging `share/unshare` 进行方向相关复制。
//! 当前内核恒等映射下 DMA 物理地址和 CPU 地址数值一致，但普通 frame allocator 不会返回
//! DMA pool 中的页。

#![no_std]
extern crate alloc;

use core::ptr::NonNull;

use api_v0::{BlockDevice, DriverError, DriverResult, Lba};
use common::virtio_hal::VirtioHal;
use driver_api::MmioRegion;
use virtio_drivers::device::blk::VirtIOBlk;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::PAGE_SIZE;

const _ : () = assert!(PAGE_SIZE == mm_api::addr::PAGE_SIZE);
const IOZONE_PROBE_MIN_WRITE_BYTES : usize = 4096;

/// 将统一 DMA pool HAL 接到 `virtio-drivers`。
/* 共享 HAL 由 driver-impl-common 提供；旧的本地实现仅作为历史参考保留。 */
/*
    /// 该实现由公共 HAL 提供；此类型保留用于兼容现有驱动类型参数。
    fn dma_alloc(pages : usize, _direction : BufferDirection) -> (PhysAddr, NonNull<u8>) {
        if pages == 0 {
            return (0, NonNull::dangling());
        }
        let mut ppns : Vec<PhysPageNum> = Vec::new();
        for _ in 0..pages {
            match frame_alloc_result() {
                Ok(p) => ppns.push(p),
                Err(_) => {
                    for q in ppns {
                        let _ = frame_dealloc_result(q);
                    }
                    logging::error!("[virtio-blk-hal] dma_alloc: frame pool OOM (pages={})",
                                    pages);
                    return (0, NonNull::dangling());
                }
            }
        }
        // 栈式分配器顺序分配得到 **物理页号递减** 的连续页：p, p-1, …
        for i in 1..pages {
            if ppns[i - 1].0 != ppns[i].0 + 1 {
                for q in ppns {
                    let _ = frame_dealloc_result(q);
                }
                logging::error!("[virtio-blk-hal] dma_alloc: non-contiguous frames (expect stack \
                                 PPNs)");
                return (0, NonNull::dangling());
            }
        }
        let base_ppn = ppns[pages - 1].0;
        let Some(paddr_us) = base_ppn.checked_mul(PAGE_SIZE) else {
            for q in ppns {
                let _ = frame_dealloc_result(q);
            }
            return (0, NonNull::dangling());
        };
        let paddr = paddr_us as PhysAddr;
        let vptr = paddr_us as *mut u8;
        unsafe {
            ptr::write_bytes(vptr, 0, pages * PAGE_SIZE);
        }
        let Some(nn) = NonNull::new(vptr) else {
            for q in ppns {
                let _ = frame_dealloc_result(q);
            }
            return (0, NonNull::dangling());
        };
        (paddr, nn)
    }

    unsafe fn dma_dealloc(paddr : PhysAddr, vaddr : NonNull<u8>, pages : usize) -> i32 {
        if pages == 0 || paddr == 0 {
            return 0;
        }
        debug_assert_eq!(vaddr.as_ptr() as usize,
                         paddr as usize,
                         "identity DMA: vaddr must match paddr");
        let base_ppn = (paddr as usize) / PAGE_SIZE;
        for i in 0..pages {
            let _ = frame_dealloc_result(PhysPageNum(base_ppn + i));
        }
        0
    }

    unsafe fn mmio_phys_to_virt(paddr : PhysAddr, _size : usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("mmio_phys_to_virt: null")
    }

    unsafe fn share(buffer : NonNull<[u8]>, _direction : BufferDirection) -> PhysAddr {
        let ptr = buffer.as_ptr() as *mut u8 as usize;
        ptr as PhysAddr
    }

    unsafe fn unshare(_paddr : PhysAddr, _buffer : NonNull<[u8]>, _direction : BufferDirection) {}
}
*/

/// VirtIO-MMIO 上的块设备（`virtio-blk`）。
pub struct VirtioBlkDevice {
    /// `virtio-drivers` 侧已握手的传输与队列状态。
    inner : VirtIOBlk<VirtioHal, MmioTransport<'static>>,
}

impl VirtioBlkDevice {
    /// 在给定 MMIO 窗口内探测并初始化 `virtio-blk`；头指针或传输握手失败时映射为 [`DriverError`]。
    ///
    /// **须在** `init_frame_allocator`（或等价全局帧池初始化）**之后**调用。
    pub fn from_mmio(mmio : MmioRegion) -> DriverResult<Self> {
        // 先拒绝空基址，再建立带长度检查的 MMIO transport，避免把非法 DTB 地址交给驱动。
        let header = NonNull::new(mmio.base as *mut VirtIOHeader).ok_or(DriverError::InvalidDtb)?;
        let transport =
            unsafe { MmioTransport::new(header, mmio.size) }.map_err(|_| DriverError::Unsupported)?;
        let inner =
            VirtIOBlk::<VirtioHal, MmioTransport>::new(transport).map_err(|_| {
                                                                     DriverError::Unsupported
                                                                 })?;
        Ok(Self { inner })
    }
}

impl BlockDevice for VirtioBlkDevice {
    fn total_blocks(&self) -> Option<u64> {
        Some(self.inner
                 .capacity())
    }

    /// 以 LBA 为单位读入 `buf`；长度须为块大小的整数倍，否则由 VirtIO 层返回错误。
    fn read_blocks(&mut self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
        // 先做容量、整块长度和 LBA 溢出检查，再转换为 virtio-drivers 使用的 usize。
        self.check_request_range(start_block, buf.len())?;
        let start = usize::try_from(start_block.0).map_err(|_| DriverError::InvalidParam)?;
        self.inner
            .read_blocks(start, buf)
            .map_err(|_| DriverError::IoError)
    }

    /// 将 `buf` 写回磁盘；语义与 [`read_blocks`] 对称。
    fn write_blocks(&mut self, start_block : Lba, buf : &[u8]) -> DriverResult<()> {
        self.check_request_range(start_block, buf.len())?;
        let start = usize::try_from(start_block.0).map_err(|_| DriverError::InvalidParam)?;
        let probe = buf.len() >= IOZONE_PROBE_MIN_WRITE_BYTES;
        if probe {
            logging::trace!("[virtio-blk-write] begin lba={} bytes={}",
                            start_block.0,
                            buf.len());
        }
        let result = self.inner
                         .write_blocks(start, buf)
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

    fn flush(&mut self) -> DriverResult<()> {
        self.inner
            .flush()
            .map_err(|_| DriverError::IoError)
    }
}
