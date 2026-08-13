//! RISC-V QEMU VirtIO-MMIO GPU 实现。

#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use core::{ptr, ptr::NonNull, slice};

use api_v0::{DisplayDevice, DriverError, DriverResult, FramebufferInfo, PixelFormat};
use driver_api::MmioRegion;
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PhysPageNum;
use virtio_drivers::device::gpu::VirtIOGpu;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

const _ : () = assert!(PAGE_SIZE == mm_api::addr::PAGE_SIZE);

/// 将 WaterOS 恒等映射帧分配器接到 VirtIO GPU。
struct VirtioGpuMmioHal;

unsafe impl Hal for VirtioGpuMmioHal {
    fn dma_alloc(pages : usize, _direction : BufferDirection) -> (PhysAddr, NonNull<u8>) {
        if pages == 0 {
            return (0, NonNull::dangling());
        }
        let mut ppns = Vec::new();
        for _ in 0..pages {
            match frame_alloc_result() {
                Ok(ppn) => ppns.push(ppn),
                Err(_) => {
                    for ppn in ppns {
                        let _ = frame_dealloc_result(ppn);
                    }
                    logging::error!("[virtio-gpu-mmio] DMA allocation failed pages={}",
                                    pages);
                    return (0, NonNull::dangling());
                }
            }
        }
        for index in 1..pages {
            if ppns[index - 1].0 != ppns[index].0 + 1 {
                for ppn in ppns {
                    let _ = frame_dealloc_result(ppn);
                }
                logging::error!("[virtio-gpu-mmio] DMA frames are not contiguous");
                return (0, NonNull::dangling());
            }
        }
        let address = ppns[pages - 1].0
                                     .saturating_mul(PAGE_SIZE);
        if address == 0 {
            return (0, NonNull::dangling());
        }
        unsafe { ptr::write_bytes(address as *mut u8, 0, pages * PAGE_SIZE) };
        let Some(pointer) = NonNull::new(address as *mut u8) else {
            return (0, NonNull::dangling());
        };
        (address as PhysAddr, pointer)
    }

    unsafe fn dma_dealloc(paddr : PhysAddr, vaddr : NonNull<u8>, pages : usize) -> i32 {
        if pages == 0 || paddr == 0 {
            return 0;
        }
        debug_assert_eq!(paddr as usize, vaddr.as_ptr() as usize);
        let base = paddr as usize / PAGE_SIZE;
        for offset in 0..pages {
            let _ = frame_dealloc_result(PhysPageNum(base + offset));
        }
        0
    }

    unsafe fn mmio_phys_to_virt(paddr : PhysAddr, _size : usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("virtio-gpu MMIO address is null")
    }
    unsafe fn share(buffer : NonNull<[u8]>, _direction : BufferDirection) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize as PhysAddr
    }

    unsafe fn unshare(_paddr : PhysAddr, _buffer : NonNull<[u8]>, _direction : BufferDirection) {}
}

/// 使用 `virtio-gpu-device` 的显示设备。
pub struct VirtioGpuMmioDevice {
    inner : VirtIOGpu<VirtioGpuMmioHal, MmioTransport<'static>>,
    info : FramebufferInfo,
}

impl VirtioGpuMmioDevice {
    /// 从 DTB 枚举得到的 VirtIO-MMIO 窗口初始化 GPU 并创建 framebuffer。
    pub fn from_mmio(mmio : MmioRegion) -> DriverResult<Self> {
        let header = NonNull::new(mmio.base as *mut VirtIOHeader).ok_or(DriverError::InvalidParam)?;
        let transport =
            unsafe { MmioTransport::new(header, mmio.size) }.map_err(|_| DriverError::Unsupported)?;
        let mut inner =
            VirtIOGpu::<VirtioGpuMmioHal, MmioTransport>::new(transport).map_err(|_| {
                                                                            DriverError::Unsupported
                                                                        })?;
        let (width, height) = inner.resolution()
                                   .map_err(|_| DriverError::IoError)?;
        let framebuffer = inner.setup_framebuffer()
                               .map_err(|_| DriverError::IoError)?;
        let stride = width as usize * 4;
        let byte_len = stride.checked_mul(height as usize)
                             .ok_or(DriverError::InvalidParam)?;
        let mapped_len = byte_len.checked_add(PAGE_SIZE - 1)
                                 .ok_or(DriverError::InvalidParam)? /
                         PAGE_SIZE * PAGE_SIZE;
        if framebuffer.len() < byte_len {
            return Err(DriverError::IoError);
        }
        let info = FramebufferInfo { width,
                                     height,
                                     stride,
                                     format : PixelFormat::Bgra8888,
                                     byte_len,
                                     phys_base : framebuffer.as_mut_ptr() as usize,
                                     mapped_len,
                                     base : framebuffer.as_mut_ptr() as usize };
        Ok(Self { inner, info })
    }
}

impl DisplayDevice for VirtioGpuMmioDevice {
    fn info(&self) -> FramebufferInfo { self.info }

    fn framebuffer(&mut self) -> DriverResult<&mut [u8]> {
        // SAFETY：`inner` 持有 framebuffer DMA；设备通过 Mutex 独占借用，地址和长度
        // 在初始化后保持不变，直到本对象析构。
        Ok(unsafe {
            slice::from_raw_parts_mut(self.info.base as *mut u8,
                                      self.info.byte_len)
        })
    }

    fn flush(&mut self) -> DriverResult<()> {
        self.inner
            .flush()
            .map_err(|_| DriverError::IoError)
    }
}
