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
#[cfg(feature = "interrupt")]
use alloc::sync::Arc;
#[cfg(feature = "interrupt")]
use core::sync::atomic::{AtomicUsize, Ordering};
use core::ptr;
use core::ptr::NonNull;

use api_v0::{BlockDevice, DriverError, DriverResult, Lba};
use driver_api::MmioRegion;
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PhysPageNum;
use virtio_drivers::device::blk::VirtIOBlk;
#[cfg(feature = "interrupt")]
use virtio_drivers::device::blk::{BlkReq, BlkResp};
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
    /// `virtio-drivers` 侧已握手的传输与队列状态。
    inner: VirtIOBlk<VirtioMmioHal, MmioTransport<'static>>,
    #[cfg(feature = "interrupt")]
    interrupt: Option<Arc<InterruptCompletion>>,
}

#[cfg(feature = "interrupt")]
struct InterruptCompletion {
    generation: AtomicUsize,
}

#[cfg(feature = "interrupt")]
impl InterruptCompletion {
    fn new() -> Self {
        Self { generation: AtomicUsize::new(0) }
    }

    fn signal(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }
}

impl VirtioBlkDevice {
    /// 在给定 MMIO 窗口内探测并初始化 `virtio-blk`；头指针或传输握手失败时映射为 [`DriverError`]。
    ///
    /// **须在** `init_frame_allocator`（或等价全局帧池初始化）**之后**调用。
    pub fn from_mmio(mmio: MmioRegion) -> DriverResult<Self> {
        let header = NonNull::new(mmio.base as *mut VirtIOHeader).ok_or(DriverError::InvalidDtb)?;
        let transport =
            unsafe { MmioTransport::new(header, mmio.size) }.map_err(|_| DriverError::Unsupported)?;
        let inner = VirtIOBlk::<VirtioMmioHal, MmioTransport>::new(transport)
            .map_err(|_| DriverError::Unsupported)?;
        Ok(Self {
            inner,
            #[cfg(feature = "interrupt")]
            interrupt : None,
        })
    }

    /// 初始化 VirtIO 块设备，并使用 `hwirq` 通知请求完成；注册失败时保留轮询模式。
    #[cfg(feature = "interrupt")]
    pub fn from_mmio_with_irq(mmio : MmioRegion, hwirq : u32) -> DriverResult<Self> {
        let mut device = Self::from_mmio(mmio)?;
        let completion = Arc::new(InterruptCompletion::new());
        let signal = completion.clone();
        let base = mmio.base;
        match irq::request(hwirq,
                           false,
                           move |_| {
                               // VirtIO-MMIO InterruptStatus/InterruptACK 位于 0x60/0x64。
                               // ISR 不获取块设备锁，避免与睡眠等待完成的提交线程锁反转。
                               let status = unsafe {
                                   core::ptr::read_volatile((base + 0x60) as *const u32)
                               };
                               if status == 0 {
                                   // Another delivery may already have acknowledged this level
                                   // interrupt before the PLIC completion becomes visible.
                                   return irq::IrqReturn::Handled;
                               }
                               unsafe {
                                   core::ptr::write_volatile((base + 0x64) as *mut u32, status)
                               };
                               signal.signal();
                               irq::IrqReturn::Handled
                           })
        {
            Ok(_) => {
                device.inner.enable_interrupts();
                device.interrupt = Some(completion);
                logging::info!("[virtio-blk] interrupt completion enabled irq={}", hwirq);
            }
            Err(error) => {
                logging::warn!("[virtio-blk] IRQ {} registration failed ({:?}); retaining polling mode",
                               hwirq,
                               error);
            }
        }
        Ok(device)
    }

    #[cfg(feature = "interrupt")]
    fn wait_for_token(&mut self, token : u16, completion : &InterruptCompletion) {
        loop {
            if self.inner.peek_used() == Some(token) {
                return;
            }
            let observed = completion.generation.load(Ordering::Acquire);
            if self.inner.peek_used() == Some(token) {
                return;
            }
            irq::wait_for_interrupt(|| {
                completion.generation.load(Ordering::Acquire) != observed ||
                self.inner.peek_used() == Some(token)
            });
        }
    }

    #[cfg(feature = "interrupt")]
    fn read_blocks_interrupt(&mut self,
                             start_block : Lba,
                             buf : &mut [u8])
                             -> DriverResult<()> {
        let completion = self.interrupt
                             .as_ref()
                             .cloned()
                             .ok_or(DriverError::Unsupported)?;
        let mut request = BlkReq::default();
        let mut response = BlkResp::default();
        let token = unsafe {
            self.inner.read_blocks_nb(start_block.0 as usize,
                                      &mut request,
                                      buf,
                                      &mut response)
        }.map_err(|_| DriverError::IoError)?;
        self.wait_for_token(token, &completion);
        unsafe { self.inner.complete_read_blocks(token, &request, buf, &mut response) }
            .map_err(|_| DriverError::IoError)
    }

    #[cfg(feature = "interrupt")]
    fn write_blocks_interrupt(&mut self,
                              start_block : Lba,
                              buf : &[u8])
                              -> DriverResult<()> {
        let completion = self.interrupt
                             .as_ref()
                             .cloned()
                             .ok_or(DriverError::Unsupported)?;
        let mut request = BlkReq::default();
        let mut response = BlkResp::default();
        let token = unsafe {
            self.inner.write_blocks_nb(start_block.0 as usize,
                                       &mut request,
                                       buf,
                                       &mut response)
        }.map_err(|_| DriverError::IoError)?;
        self.wait_for_token(token, &completion);
        unsafe { self.inner.complete_write_blocks(token, &request, buf, &mut response) }
            .map_err(|_| DriverError::IoError)
    }
}

impl BlockDevice for VirtioBlkDevice {
    /// 以 LBA 为单位读入 `buf`；长度须为块大小的整数倍，否则由 VirtIO 层返回错误。
    fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
        #[cfg(feature = "interrupt")]
        {
            if self.interrupt.is_some() && irq::can_wait() {
                return self.read_blocks_interrupt(start_block, buf);
            }
        }
        self.inner
            .read_blocks(start_block.0 as usize, buf)
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
        #[cfg(feature = "interrupt")]
        let result = if self.interrupt.is_some() && irq::can_wait() {
            self.write_blocks_interrupt(start_block, buf)
        } else {
            self.inner.write_blocks(start_block.0 as usize, buf)
                      .map_err(|_| DriverError::IoError)
        };
        #[cfg(not(feature = "interrupt"))]
        let result = self.inner.write_blocks(start_block.0 as usize, buf)
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
