//! VirtIO 网络设备（MMIO 传输）实现，供平台驱动在枚举到 `virtio,mmio` + network 设备后实例化。
//!
//! **DMA / HAL**：与块设备共用同一套恒等映射帧分配策略。

#![no_std]
extern crate alloc;

use core::ptr::NonNull;

use api_v0::{DriverError, DriverResult, NetworkDevice, DEFAULT_MTU};
use driver_api::MmioRegion;
use driver_common::virtio_dma;
use virtio_drivers::device::net::VirtIONet;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

const _: () = assert!(PAGE_SIZE == mm_api::addr::PAGE_SIZE);

/// 接收缓冲区大小（含 virtio net header）；须不小于 `MIN_BUFFER_LEN`（1526）。
const RX_BUF_LEN: usize = 2048;

/// 将内核帧分配器接到 `virtio-drivers` 的 [`Hal`]：恒等映射下返回的 `PhysAddr` 与可写虚拟指针相同。
struct VirtioMmioHal;

unsafe impl Hal for VirtioMmioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        virtio_dma::alloc(pages)
            .map(|(address, pointer)| (address as PhysAddr, pointer))
            .unwrap_or((0, NonNull::dangling()))
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
        unsafe { virtio_dma::dealloc(paddr as u64, vaddr, pages) }
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("mmio_phys_to_virt: null")
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        unsafe { virtio_dma::share_identity(buffer) as PhysAddr }
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}

/// VirtIO-MMIO 上的网络设备（`virtio-net`）。
pub struct VirtioNetDevice {
    inner: VirtIONet<VirtioMmioHal, MmioTransport<'static>, 32>,
}

impl VirtioNetDevice {
    /// 在给定 MMIO 窗口内探测并初始化 `virtio-net`；头指针或传输握手失败时映射为 [`DriverError`]。
    ///
    /// **须在** `init_frame_allocator`（或等价全局帧池初始化）**之后**调用。
    pub fn from_mmio(mmio: MmioRegion) -> DriverResult<Self> {
        let header = NonNull::new(mmio.base as *mut VirtIOHeader).ok_or(DriverError::InvalidDtb)?;
        let transport =
            unsafe { MmioTransport::new(header, mmio.size) }.map_err(|_| DriverError::Unsupported)?;
        let inner = VirtIONet::<VirtioMmioHal, MmioTransport, 32>::new(transport, RX_BUF_LEN)
            .map_err(|_| DriverError::Unsupported)?;
        Ok(Self { inner })
    }
}

impl NetworkDevice for VirtioNetDevice {
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
                        logging::warn!("[virtio-net] recycle_rx_buffer failed: {:?}", e);
                    }
                    return Err(DriverError::InvalidParam);
                }
                let len = packet.len().min(buf.len());
                buf[..len].copy_from_slice(&packet[..len]);
                let packet_len = rx_buf.packet_len();
                // 回收缓冲区到接收队列以便下次接收。
                if let Err(e) = self.inner.recycle_rx_buffer(rx_buf) {
                    logging::warn!("[virtio-net] recycle_rx_buffer failed: {:?}", e);
                }
                Ok(packet_len.min(buf.len()))
            }
            Err(virtio_drivers::Error::NotReady) => Ok(0),
            Err(_) => Err(DriverError::IoError),
        }
    }
}
