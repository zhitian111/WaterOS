//! LoongArch QEMU VirtIO-PCI GPU 实现。

#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use core::{ptr, ptr::NonNull, slice};

use api_v0::{DisplayDevice, DriverError, DriverResult, FramebufferInfo, PixelFormat};
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PhysPageNum;
use virtio_drivers::device::gpu::VirtIOGpu;
use virtio_drivers::transport::pci::bus::{
    BarInfo, Cam, Command, ConfigurationAccess, DeviceFunction, MemoryBarType, MmioCam, PciRoot,
};
use virtio_drivers::transport::pci::{self, PciTransport};
use virtio_drivers::transport::DeviceType as VirtioDeviceType;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

const _ : () = assert!(PAGE_SIZE == mm_api::addr::PAGE_SIZE);

/// PCI 总线位置与设备 ID，供平台日志和诊断使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtioGpuPciProbeInfo {
    pub bus : u8,
    pub device : u8,
    pub function : u8,
    pub vendor_id : u16,
    pub device_id : u16,
}

impl VirtioGpuPciProbeInfo {
    fn new(df : DeviceFunction, vendor_id : u16, device_id : u16) -> Self {
        Self { bus : df.bus,
               device : df.device,
               function : df.function,
               vendor_id,
               device_id }
    }
}

/// 裸机环境为 VirtIO GPU BAR 分配 PCI MMIO 地址的单调分配器。
pub struct VirtioGpuPciBarAllocator {
    next : u64,
    end : u64,
}

impl VirtioGpuPciBarAllocator {
    pub const fn new(start : u64, end : u64) -> Self { Self { next : start, end } }

    fn allocate(&mut self, size : u64) -> Option<u64> {
        if size == 0 {
            return None;
        }
        let align = size.checked_next_power_of_two()?
                        .max(16);
        let start = self.next
                        .checked_add(align - 1)? &
                    !(align - 1);
        let end = start.checked_add(size)?;
        if end > self.end {
            return None;
        }
        self.next = end;
        Some(start)
    }
}

struct VirtioGpuPciHal;

unsafe impl Hal for VirtioGpuPciHal {
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
                    logging::error!("[virtio-gpu-pci] DMA allocation failed pages={}",
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
                logging::error!("[virtio-gpu-pci] DMA frames are not contiguous");
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
        NonNull::new(paddr as *mut u8).expect("virtio-gpu PCI address is null")
    }

    unsafe fn share(buffer : NonNull<[u8]>, _direction : BufferDirection) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize as PhysAddr
    }

    unsafe fn unshare(_paddr : PhysAddr, _buffer : NonNull<[u8]>, _direction : BufferDirection) {}
}

/// 使用 `virtio-gpu-pci` transport 的显示设备。
pub struct VirtioGpuPciDevice {
    inner : VirtIOGpu<VirtioGpuPciHal, PciTransport>,
    info : FramebufferInfo,
}

impl VirtioGpuPciDevice {
    fn from_pci_root<C : ConfigurationAccess>(root : &mut PciRoot<C>,
                                              df : DeviceFunction,
                                              allocator : &mut VirtioGpuPciBarAllocator)
                                              -> DriverResult<Self> {
        assign_memory_bars(root, df, allocator)?;
        let (_status, command) = root.get_status_command(df);
        root.set_command(df,
                         command | Command::MEMORY_SPACE | Command::BUS_MASTER);
        let transport =
            PciTransport::new::<VirtioGpuPciHal, C>(root, df).map_err(|_| {
                                                                 DriverError::Unsupported
                                                             })?;
        let mut inner =
            VirtIOGpu::<VirtioGpuPciHal, PciTransport>::new(transport).map_err(|_| {
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

    /// 扫描 PCI bus 0，并初始化第一个 VirtIO GPU。
    ///
    /// # Safety
    ///
    /// `config_base` 必须指向内核可访问的 PCIe ECAM 配置空间。
    pub unsafe fn probe_first_from_ecam(config_base : usize,
                                        allocator : &mut VirtioGpuPciBarAllocator)
                                        -> DriverResult<Option<(Self, VirtioGpuPciProbeInfo)>> {
        let cam = unsafe { MmioCam::new(config_base as *mut u8, Cam::Ecam) };
        let mut root = PciRoot::new(cam);
        for (df, info) in root.enumerate_bus(0) {
            if pci::virtio_device_type(&info) != Some(VirtioDeviceType::GPU) {
                continue;
            }
            let probe = VirtioGpuPciProbeInfo::new(df, info.vendor_id, info.device_id);
            let device = Self::from_pci_root(&mut root, df, allocator)?;
            return Ok(Some((device, probe)));
        }
        Ok(None)
    }
}

impl DisplayDevice for VirtioGpuPciDevice {
    fn info(&self) -> FramebufferInfo { self.info }

    fn framebuffer(&mut self) -> DriverResult<&mut [u8]> {
        // SAFETY：DMA 对象由 `inner` 持有，且设备 Mutex 保证这里是唯一可写借用。
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

fn assign_memory_bars<C : ConfigurationAccess>(root : &mut PciRoot<C>,
                                               df : DeviceFunction,
                                               allocator : &mut VirtioGpuPciBarAllocator)
                                               -> DriverResult<()> {
    let bars = root.bars(df)
                   .map_err(|_| DriverError::Unsupported)?;
    let mut index = 0usize;
    while index < bars.len() {
        let Some(ref bar) = bars[index] else {
            index += 1;
            continue;
        };
        let takes_two = bar.takes_two_entries();
        if let BarInfo::Memory { address_type, size, .. } = *bar {
            if address_type == MemoryBarType::Below1MiB {
                return Err(DriverError::Unsupported);
            }
            let assigned = allocator.allocate(size)
                                    .ok_or(DriverError::Unsupported)?;
            match address_type {
                MemoryBarType::Width32 => {
                    let address = u32::try_from(assigned).map_err(|_| DriverError::Unsupported)?;
                    root.set_bar_32(df, index as u8, address);
                }
                MemoryBarType::Width64 => root.set_bar_64(df, index as u8, assigned),
                MemoryBarType::Below1MiB => unreachable!(),
            }
        }
        index += if takes_two { 2 } else { 1 };
    }
    Ok(())
}
