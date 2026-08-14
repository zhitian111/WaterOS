//! VirtIO 块设备（PCI 传输）实现，供 QEMU LoongArch64 `virtio-blk-pci` 等路径使用。
//!
//! PCI BAR 承载 VirtIO capability 区域，而非 MMIO 寄存器文件；DMA 契约与
//! [`impl-virtio-mmio`] 一致：帧分配器页在恒等映射下 `paddr == vaddr`。

#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use core::ptr;
use core::ptr::NonNull;

use api_v0::{BlockDevice, DriverError, DriverResult, Lba};
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PhysPageNum;
use virtio_drivers::device::blk::VirtIOBlk;
use virtio_drivers::transport::pci::bus::{
    BarInfo, Cam, Command, ConfigurationAccess, DeviceFunction, MemoryBarType, MmioCam, PciRoot,
};
use virtio_drivers::transport::pci::{self, PciTransport};
use virtio_drivers::transport::DeviceType as VirtioDeviceType;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

const _: () = assert!(PAGE_SIZE == mm_api::addr::PAGE_SIZE);

/// PCI 探测成功时返回的可读位置信息（bus/device/function 与 ID）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtioPciProbeInfo {
    /// PCI bus 号。
    pub bus: u8,
    /// PCI device 号。
    pub device: u8,
    /// PCI function 号。
    pub function: u8,
    /// 配置空间 vendor id。
    pub vendor_id: u16,
    /// 配置空间 device id。
    pub device_id: u16,
}

/// PCI MMIO BAR 单调递增分配器（裸机 bring-up 无固件分配 BAR 时使用）。
#[derive(Debug, Clone, Copy)]
pub struct VirtioPciBarAllocator {
    next: u64,
    end: u64,
}

impl VirtioPciBarAllocator {
    /// 在 `[start, end)` 区间内分配 BAR 物理地址。
    pub const fn new(start: u64, end: u64) -> Self {
        Self { next: start, end }
    }

    fn allocate(&mut self, size: u64) -> Option<u64> {
        if size == 0 {
            return None;
        }
        let align = size.next_power_of_two().max(16);
        let start = align_up_u64(self.next, align)?;
        let end = start.checked_add(size)?;
        if end > self.end {
            return None;
        }
        self.next = end;
        Some(start)
    }
}

impl VirtioPciProbeInfo {
    fn new(df: DeviceFunction, vendor_id: u16, device_id: u16) -> Self {
        Self {
            bus: df.bus,
            device: df.device,
            function: df.function,
            vendor_id,
            device_id,
        }
    }
}

fn align_up_u64(value: u64, align: u64) -> Option<u64> {
    debug_assert!(align.is_power_of_two());
    value.checked_add(align - 1).map(|v| v & !(align - 1))
}

struct VirtioPciHal;

unsafe impl Hal for VirtioPciHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        if pages == 0 {
            return (0, NonNull::dangling());
        }
        let mut ppns: Vec<PhysPageNum> = Vec::new();
        for _ in 0..pages {
            match frame_alloc_result() {
                Ok(p) => ppns.push(p),
                Err(_) => {
                    for q in ppns {
                        let _ = frame_dealloc_result(q);
                    }
                    logging::error!(
                        "[virtio-pci-hal] dma_alloc: frame pool OOM (pages={})",
                        pages
                    );
                    return (0, NonNull::dangling());
                }
            }
        }
        // StackFrameAllocator returns descending contiguous PPNs for fresh multi-page bursts.
        for i in 1..pages {
            if ppns[i - 1].0 != ppns[i].0 + 1 {
                for q in ppns {
                    let _ = frame_dealloc_result(q);
                }
                logging::error!("[virtio-pci-hal] dma_alloc: non-contiguous frames");
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
        unsafe {
            ptr::write_bytes(
                paddr_us as *mut u8,
                0,
                pages * PAGE_SIZE,
            );
        }
        let Some(vaddr) = NonNull::new(paddr_us as *mut u8) else {
            for q in ppns {
                let _ = frame_dealloc_result(q);
            }
            return (0, NonNull::dangling());
        };
        (paddr_us as PhysAddr, vaddr)
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
        if pages == 0 || paddr == 0 {
            return 0;
        }
        debug_assert_eq!(
            vaddr.as_ptr() as usize,
            paddr as usize,
            "identity DMA: vaddr must match paddr"
        );
        let base_ppn = (paddr as usize) / PAGE_SIZE;
        for i in 0..pages {
            let _ = frame_dealloc_result(PhysPageNum(base_ppn + i));
        }
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("mmio_phys_to_virt: null")
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize as PhysAddr
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}

/// VirtIO block device backed by PCI transport.
pub struct VirtioPciBlkDevice {
    inner: VirtIOBlk<VirtioPciHal, PciTransport>,
}

impl VirtioPciBlkDevice {
    /// Initialize a block device from an already discovered PCI device function.
    pub fn from_pci_root<C: ConfigurationAccess>(
        root: &mut PciRoot<C>,
        device_function: DeviceFunction,
        bar_allocator: &mut VirtioPciBarAllocator,
    ) -> DriverResult<Self> {
        assign_memory_bars(root, device_function, bar_allocator)?;
        let (_status, command) = root.get_status_command(device_function);
        root.set_command(
            device_function,
            command | Command::MEMORY_SPACE | Command::BUS_MASTER,
        );
        let transport = PciTransport::new::<VirtioPciHal, C>(root, device_function)
            .map_err(|_| DriverError::Unsupported)?;
        let inner = VirtIOBlk::<VirtioPciHal, PciTransport>::new(transport)
            .map_err(|_| DriverError::Unsupported)?;
        Ok(Self { inner })
    }

    /// Scan PCI bus 0 in a memory-mapped PCI CAM/ECAM window and return the first VirtIO block
    /// device.
    ///
    /// # Safety
    ///
    /// `config_base` must be directly accessible by the kernel and cover the requested PCI
    /// configuration window.
    pub unsafe fn probe_first_from_config(
        config_base: usize,
        cam: Cam,
        bar_allocator: &mut VirtioPciBarAllocator,
    ) -> DriverResult<Option<(Self, VirtioPciProbeInfo)>> {
        let cam = unsafe { MmioCam::new(config_base as *mut u8, cam) };
        let mut root = PciRoot::new(cam);
        for (df, info) in root.enumerate_bus(0) {
            logging::info!(
                "[virtio-pci-blk] pci {} vendor={:#06x} device={:#06x}",
                df,
                info.vendor_id,
                info.device_id
            );
            if pci::virtio_device_type(&info) != Some(VirtioDeviceType::Block) {
                continue;
            }
            let probe = VirtioPciProbeInfo::new(df, info.vendor_id, info.device_id);
            let dev = Self::from_pci_root(&mut root, df, bar_allocator)?;
            return Ok(Some((dev, probe)));
        }
        Ok(None)
    }

    /// Scan PCI bus 0 in an ECAM window and return the first VirtIO block device.
    ///
    /// # Safety
    ///
    /// `ecam_base` must be directly accessible by the kernel and cover a PCIe ECAM region.
    pub unsafe fn probe_first_from_ecam(
        ecam_base: usize,
        bar_allocator: &mut VirtioPciBarAllocator,
    ) -> DriverResult<Option<(Self, VirtioPciProbeInfo)>> {
        unsafe { Self::probe_first_from_config(ecam_base, Cam::Ecam, bar_allocator) }
    }

    /// Scan PCI bus 0 in a legacy memory-mapped CAM window and return the first VirtIO block
    /// device.
    ///
    /// # Safety
    ///
    /// `cam_base` must be directly accessible by the kernel and cover a PCI CAM region.
    pub unsafe fn probe_first_from_mmio_cam(
        cam_base: usize,
        bar_allocator: &mut VirtioPciBarAllocator,
    ) -> DriverResult<Option<(Self, VirtioPciProbeInfo)>> {
        unsafe { Self::probe_first_from_config(cam_base, Cam::MmioCam, bar_allocator) }
    }
}

fn assign_memory_bars<C: ConfigurationAccess>(
    root: &mut PciRoot<C>,
    device_function: DeviceFunction,
    allocator: &mut VirtioPciBarAllocator,
) -> DriverResult<()> {
    let bars = root
        .bars(device_function)
        .map_err(|_| DriverError::Unsupported)?;
    let mut bar_index = 0usize;
    while bar_index < bars.len() {
        let Some(ref bar) = bars[bar_index] else {
            bar_index += 1;
            continue;
        };
        let takes_two = bar.takes_two_entries();
        match *bar {
            BarInfo::Memory {
                address_type,
                address,
                size,
                ..
            } => {
                if address_type == MemoryBarType::Below1MiB {
                    logging::warn!(
                        "[virtio-pci-blk] unsupported below-1MiB BAR{} size={:#x}",
                        bar_index,
                        size
                    );
                    return Err(DriverError::Unsupported);
                }
                let Some(assigned) = allocator.allocate(size) else {
                    logging::warn!(
                        "[virtio-pci-blk] no PCI MMIO space for BAR{} size={:#x}",
                        bar_index,
                        size
                    );
                    return Err(DriverError::Unsupported);
                };
                match address_type {
                    MemoryBarType::Width32 => {
                        if assigned > u32::MAX as u64 {
                            return Err(DriverError::Unsupported);
                        }
                        root.set_bar_32(device_function, bar_index as u8, assigned as u32);
                    }
                    MemoryBarType::Width64 => {
                        root.set_bar_64(device_function, bar_index as u8, assigned);
                    }
                    MemoryBarType::Below1MiB => unreachable!(),
                }
                logging::info!(
                    "[virtio-pci-blk] BAR{} memory {:#x} -> {:#x} size={:#x} type={:?}",
                    bar_index,
                    address,
                    assigned,
                    size,
                    address_type
                );
            }
            BarInfo::IO { address, size } => {
                logging::info!(
                    "[virtio-pci-blk] BAR{} I/O left disabled address={:#x} size={:#x}",
                    bar_index,
                    address,
                    size
                );
            }
        }
        bar_index += if takes_two { 2 } else { 1 };
    }
    Ok(())
}

impl BlockDevice for VirtioPciBlkDevice {
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
