//! VirtIO 块设备（MMIO 传输）实现，供平台驱动在枚举到 `virtio,mmio` + block 设备后实例化。
//!
//! **DMA / HAL**：`virtio-drivers` 的队列与内部缓冲通过 [`Hal::dma_alloc`] /
//! [`Hal::dma_dealloc`] 向本机要 **物理连续、页对齐、已清零** 的内存。此前使用固定 bump
//! 物理地址且 `dma_dealloc` 为空实现，易与内核其它内存及 VirtIO 传输重叠；现改为使用
//! 已初始化的全局 **帧分配器**（`wateros-mm-frame-alloctor`），与 Sv39 bring-up 一致。
//! 恒等映射下 `paddr == vaddr`（`usize`/`PhysAddr` 视图一致）。

#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use alloc::boxed::Box;
use core::cell::UnsafeCell;
use core::ptr;
use core::ptr::NonNull;
#[cfg(feature = "interrupt-wait")]
use core::sync::atomic::{AtomicBool, Ordering};

use api_v0::{BlockDevice, DriverError, DriverResult, Lba};
#[cfg(feature = "interrupt-wait")]
use driver_api::interrupt::{self, IrqHandled};
use driver_api::interrupt::IrqNumber;
use driver_api::MmioRegion;
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PhysPageNum;
#[cfg(feature = "interrupt-wait")]
use virtio_drivers::device::blk::{BlkReq, BlkResp};
use virtio_drivers::device::blk::VirtIOBlk;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

const _: () = assert!(PAGE_SIZE == mm_api::addr::PAGE_SIZE);
const IOZONE_PROBE_MIN_WRITE_BYTES: usize = 4096;

/// 将内核帧分配器接到 `virtio-drivers` 的 [`Hal`]：恒等映射下返回的 `PhysAddr` 与可写虚拟指针相同。
struct VirtioMmioHal;

unsafe impl Hal for VirtioMmioHal {
    /// 按页数向帧池要连续物理页；失败时释放已拿页并返回空指针对，由上层映射为 [`DriverError`]。
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
                    logging::error!("[virtio-blk-hal] dma_alloc: frame pool OOM (pages={})", pages);
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
                logging::error!(
                    "[virtio-blk-hal] dma_alloc: non-contiguous frames (expect stack PPNs)"
                );
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
        let ptr = buffer.as_ptr() as *mut u8 as usize;
        ptr as PhysAddr
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}

/// VirtIO-MMIO 上的块设备（`virtio-blk`）。
pub struct VirtioBlkDevice {
    state: Box<IrqState>,
}

struct IrqState {
    inner: UnsafeCell<VirtIOBlk<VirtioMmioHal, MmioTransport<'static>>>,
    #[cfg(feature = "interrupt-wait")]
    mmio_base: usize,
    #[cfg(feature = "interrupt-wait")]
    fired: AtomicBool,
    #[cfg(feature = "interrupt-wait")]
    wait: ipc_waitqueue::WaitQueue,
}

// BlockDevice's outer registry mutex serializes queue requests.  The IRQ
// handler never aliases `inner`; it only accesses the transport's independent
// MMIO interrupt status/ack registers before publishing `fired`.
unsafe impl Send for IrqState {}

#[cfg(feature = "interrupt-wait")]
unsafe fn block_irq_handler(_irq: IrqNumber, context: usize) -> IrqHandled {
    let state = unsafe { &*(context as *const IrqState) };
    const INTERRUPT_STATUS: usize = 0x60;
    const INTERRUPT_ACK: usize = 0x64;
    let status = unsafe {
        core::ptr::read_volatile((state.mmio_base + INTERRUPT_STATUS) as *const u32)
    };
    if status == 0 {
        return IrqHandled::No;
    }
    unsafe {
        core::ptr::write_volatile((state.mmio_base + INTERRUPT_ACK) as *mut u32, status);
    }
    state.fired.store(true, Ordering::Release);
    state.wait.wake_one();
    IrqHandled::Yes
}

impl VirtioBlkDevice {
    /// 在给定 MMIO 窗口内探测并初始化 `virtio-blk`；头指针或传输握手失败时映射为 [`DriverError`]。
    ///
    /// **须在** `init_frame_allocator`（或等价全局帧池初始化）**之后**调用。
    pub fn from_mmio(mmio: MmioRegion, irq: Option<IrqNumber>) -> DriverResult<Self> {
        let header = NonNull::new(mmio.base as *mut VirtIOHeader).ok_or(DriverError::InvalidDtb)?;
        let transport =
            unsafe { MmioTransport::new(header, mmio.size) }.map_err(|_| DriverError::Unsupported)?;
        #[allow(unused_mut)]
        let mut inner = VirtIOBlk::<VirtioMmioHal, MmioTransport>::new(transport)
            .map_err(|_| DriverError::Unsupported)?;
        #[cfg(feature = "interrupt-wait")]
        inner.enable_interrupts();
        let state = Box::new(IrqState {
            inner: UnsafeCell::new(inner),
            #[cfg(feature = "interrupt-wait")]
            mmio_base: mmio.base,
            #[cfg(feature = "interrupt-wait")]
            fired: AtomicBool::new(false),
            #[cfg(feature = "interrupt-wait")]
            wait: ipc_waitqueue::WaitQueue::new_named("virtio-blk"),
        });
        #[cfg(feature = "interrupt-wait")]
        if let Some(irq) = irq {
            let context = state.as_ref() as *const IrqState as usize;
            if !unsafe { interrupt::register_handler(irq, block_irq_handler, context) } {
                return Err(DriverError::Unsupported);
            }
        }
        #[cfg(not(feature = "interrupt-wait"))]
        let _ = irq;
        Ok(Self { state })
    }

    fn inner(&mut self) -> &mut VirtIOBlk<VirtioMmioHal, MmioTransport<'static>> {
        unsafe { &mut *self.state.inner.get() }
    }

    #[cfg(feature = "interrupt-wait")]
    fn wait_read(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
        let mut req = BlkReq::default();
        let mut resp = BlkResp::default();
        self.state.fired.store(false, Ordering::Release);
        let token = unsafe {
            self.inner().read_blocks_nb(start_block.0 as usize, &mut req, buf, &mut resp)
        }.map_err(|_| DriverError::IoError)?;
        while self.inner().peek_used() != Some(token) {
            self.state.wait.wait_current_while(|| {
                !self.state.fired.swap(false, Ordering::AcqRel)
            });
        }
        let result = unsafe { self.inner().complete_read_blocks(token, &req, buf, &mut resp) }
            .map_err(|_| DriverError::IoError);
        result
    }

    #[cfg(feature = "interrupt-wait")]
    fn wait_write(&mut self, start_block: Lba, buf: &[u8]) -> DriverResult<()> {
        let mut req = BlkReq::default();
        let mut resp = BlkResp::default();
        self.state.fired.store(false, Ordering::Release);
        let token = unsafe {
            self.inner().write_blocks_nb(start_block.0 as usize, &mut req, buf, &mut resp)
        }.map_err(|_| DriverError::IoError)?;
        while self.inner().peek_used() != Some(token) {
            self.state.wait.wait_current_while(|| {
                !self.state.fired.swap(false, Ordering::AcqRel)
            });
        }
        let result = unsafe { self.inner().complete_write_blocks(token, &req, buf, &mut resp) }
            .map_err(|_| DriverError::IoError);
        result
    }
}

impl BlockDevice for VirtioBlkDevice {
    /// 以 LBA 为单位读入 `buf`；长度须为块大小的整数倍，否则由 VirtIO 层返回错误。
    fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
        #[cfg(feature = "interrupt-wait")]
        if interrupt::runtime_dispatch_ready() {
            return self.wait_read(start_block, buf);
        }
        self.inner().read_blocks(start_block.0 as usize, buf)
            .map_err(|_| DriverError::IoError)
    }

    /// 将 `buf` 写回磁盘；语义与 [`read_blocks`] 对称。
    fn write_blocks(&mut self, start_block: Lba, buf: &[u8]) -> DriverResult<()> {
        let probe = buf.len() >= IOZONE_PROBE_MIN_WRITE_BYTES;
        if probe {
            logging::trace!("[virtio-blk-write] begin lba={} bytes={}",
                            start_block.0,
                            buf.len());
        }
        #[cfg(feature = "interrupt-wait")]
        if interrupt::runtime_dispatch_ready() {
            return self.wait_write(start_block, buf);
        }
        let result = self.inner().write_blocks(start_block.0 as usize, buf)
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
