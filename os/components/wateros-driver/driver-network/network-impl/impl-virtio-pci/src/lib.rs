//! VirtIO 网络设备（PCI transport）实现，用于 QEMU LoongArch64 `virtio-net-pci`。

#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use core::ptr;
use core::ptr::NonNull;

use api_v0::{DriverError, DriverResult, NetworkDevice, DEFAULT_MTU};
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PhysPageNum;
use virtio_drivers::device::net::VirtIONet;
use virtio_drivers::transport::pci::bus::{
    BarInfo, Cam, Command, ConfigurationAccess, DeviceFunction, MemoryBarType, MmioCam, PciRoot,
};
use virtio_drivers::transport::pci::{self, PciTransport};
use virtio_drivers::transport::DeviceType as VirtioDeviceType;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

const _: () = assert!(PAGE_SIZE == mm_api::addr::PAGE_SIZE);

/// 接收缓冲区大小（含 virtio net header）；须不小于以太网 MTU + virtio header。
const RX_BUF_LEN: usize = 2048;

/// PCI 探测成功时返回的 virtio-net 位置信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtioNetPciProbeInfo {
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

/// PCI MMIO BAR 单调递增分配器（网卡 BAR 与块设备分区不同起始地址）。
#[derive(Debug, Clone, Copy)]
pub struct VirtioNetPciBarAllocator {
    next: u64,
    end: u64,
}

impl VirtioNetPciBarAllocator {
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

impl VirtioNetPciProbeInfo {
    #[inline]
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

struct VirtioPciNetHal;

unsafe impl Hal for VirtioPciNetHal {
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
                        "[virtio-pci-net-hal] dma_alloc: frame pool OOM (pages={})",
                        pages
                    );
                    return (0, NonNull::dangling());
                }
            }
        }
        for i in 1..pages {
            if ppns[i - 1].0 != ppns[i].0 + 1 {
                for q in ppns {
                    let _ = frame_dealloc_result(q);
                }
                logging::error!("[virtio-pci-net-hal] dma_alloc: non-contiguous frames");
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
            ptr::write_bytes(paddr_us as *mut u8, 0, pages * PAGE_SIZE);
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

pub struct VirtioPciNetDevice {
    inner: VirtIONet<VirtioPciNetHal, PciTransport, 32>,
}

impl VirtioPciNetDevice {
    pub fn from_pci_root<C: ConfigurationAccess>(
        root: &mut PciRoot<C>,
        device_function: DeviceFunction,
        bar_allocator: &mut VirtioNetPciBarAllocator,
    ) -> DriverResult<Self> {
        assign_memory_bars(root, device_function, bar_allocator)?;
        let (_status, command) = root.get_status_command(device_function);
        root.set_command(
            device_function,
            command | Command::MEMORY_SPACE | Command::BUS_MASTER,
        );
        let transport = PciTransport::new::<VirtioPciNetHal, C>(root, device_function)
            .map_err(|_| DriverError::Unsupported)?;
        let inner = VirtIONet::<VirtioPciNetHal, PciTransport, 32>::new(transport, RX_BUF_LEN)
            .map_err(|_| DriverError::Unsupported)?;
        Ok(Self { inner })
    }

    /// # Safety
    ///
    /// `config_base` must be directly accessible by the kernel and cover the requested PCI
    /// configuration window.
    pub unsafe fn probe_first_from_config(
        config_base: usize,
        cam: Cam,
        bar_allocator: &mut VirtioNetPciBarAllocator,
    ) -> DriverResult<Option<(Self, VirtioNetPciProbeInfo)>> {
        let cam = unsafe { MmioCam::new(config_base as *mut u8, cam) };
        let mut root = PciRoot::new(cam);
        for (df, info) in root.enumerate_bus(0) {
            logging::info!(
                "[virtio-pci-net] pci {} vendor={:#06x} device={:#06x}",
                df,
                info.vendor_id,
                info.device_id
            );
            if pci::virtio_device_type(&info) != Some(VirtioDeviceType::Network) {
                continue;
            }
            let probe = VirtioNetPciProbeInfo::new(df, info.vendor_id, info.device_id);
            let dev = Self::from_pci_root(&mut root, df, bar_allocator)?;
            return Ok(Some((dev, probe)));
        }
        Ok(None)
    }

    /// # Safety
    ///
    /// `ecam_base` must be directly accessible by the kernel and cover a PCIe ECAM region.
    pub unsafe fn probe_first_from_ecam(
        ecam_base: usize,
        bar_allocator: &mut VirtioNetPciBarAllocator,
    ) -> DriverResult<Option<(Self, VirtioNetPciProbeInfo)>> {
        unsafe { Self::probe_first_from_config(ecam_base, Cam::Ecam, bar_allocator) }
    }
}

fn assign_memory_bars<C: ConfigurationAccess>(
    root: &mut PciRoot<C>,
    device_function: DeviceFunction,
    allocator: &mut VirtioNetPciBarAllocator,
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
                        "[virtio-pci-net] unsupported below-1MiB BAR{} size={:#x}",
                        bar_index,
                        size
                    );
                    return Err(DriverError::Unsupported);
                }
                let Some(assigned) = allocator.allocate(size) else {
                    logging::warn!(
                        "[virtio-pci-net] no PCI MMIO space for BAR{} size={:#x}",
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
                    "[virtio-pci-net] BAR{} memory {:#x} -> {:#x} size={:#x} type={:?}",
                    bar_index,
                    address,
                    assigned,
                    size,
                    address_type
                );
            }
            BarInfo::IO { address, size } => {
                logging::info!(
                    "[virtio-pci-net] BAR{} I/O left disabled address={:#x} size={:#x}",
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

impl NetworkDevice for VirtioPciNetDevice {
    fn mac_address(&self) -> [u8; 6] {
        self.inner.mac_address()
    }

    fn mtu(&self) -> usize {
        DEFAULT_MTU
    }

    fn is_link_up(&self) -> bool {
        self.inner.can_recv() || self.inner.can_send()
    }

    fn send(&mut self, buf: &[u8]) -> DriverResult<()> {
        let tx_buf = virtio_drivers::device::net::TxBuffer::from(buf);
        self.inner.send(tx_buf).map_err(|_| DriverError::IoError)
    }

    fn receive(&mut self, buf: &mut [u8]) -> DriverResult<usize> {
        match self.inner.receive() {
            Ok(rx_buf) => {
                let packet = rx_buf.packet();
                if packet.len() > buf.len() {
                    if let Err(e) = self.inner.recycle_rx_buffer(rx_buf) {
                        logging::warn!("[virtio-pci-net] recycle_rx_buffer failed: {:?}", e);
                    }
                    return Err(DriverError::InvalidParam);
                }
                let len = packet.len().min(buf.len());
                buf[..len].copy_from_slice(&packet[..len]);
                let packet_len = rx_buf.packet_len();
                if let Err(e) = self.inner.recycle_rx_buffer(rx_buf) {
                    logging::warn!("[virtio-pci-net] recycle_rx_buffer failed: {:?}", e);
                }
                Ok(packet_len.min(buf.len()))
            }
            Err(virtio_drivers::Error::NotReady) => Ok(0),
            Err(_) => Err(DriverError::IoError),
        }
    }
}
