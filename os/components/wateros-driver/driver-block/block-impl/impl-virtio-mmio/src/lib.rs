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
use core::ptr;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use spin::Once;

use api_v0::{BlockDevice, DriverError, DriverResult, Lba};
use driver_api::MmioRegion;
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PhysPageNum;
use virtio_drivers::device::blk::{BlkReq, BlkResp, VirtIOBlk};
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};
use task::WaitQueue;

const _ : () = assert!(PAGE_SIZE == mm_api::addr::PAGE_SIZE);
const IOZONE_PROBE_MIN_WRITE_BYTES : usize = 4096;

/// 单请求 IRQ 完成通知：top-half 只做 MMIO ack + 置位 + 唤醒；等待任务持设备
/// `inner` 锁期间在 waitqueue 上睡眠。当前按「单块设备」简化，使用全局标志与
/// 队列（多设备时再拆分为每设备数组）。
static IRQ_FIRED : AtomicBool = AtomicBool::new(false);
static IRQ_WAIT : Once<WaitQueue> = Once::new();
static RUNTIME_READY : AtomicBool = AtomicBool::new(false);

/// IRQ top-half 通知：置完成标志并唤醒等待任务；在 trap 上下文安全（无锁）。
pub fn notify_irq() {
    IRQ_FIRED.store(true, Ordering::Release);
    if let Some(queue) = IRQ_WAIT.get() {
        let _ = queue.wake_one();
    }
}

/// 消费完成标志；`true` 表示本周期有 IRQ 到达。
fn consume_irq() -> bool { IRQ_FIRED.swap(false, Ordering::AcqRel) }

/// 平台全局中断与 PLIC 接线就绪后调用；此前 block IRQ 等待退化为同步路径，
/// 避免 boot 阶段（全局中断尚未打开）在 IRQ 等待上永久睡眠。
pub fn enable_runtime_dispatch() {
    RUNTIME_READY.store(true, Ordering::Release);
}

fn runtime_ready() -> bool { RUNTIME_READY.load(Ordering::Acquire) }

/// 将内核帧分配器接到 `virtio-drivers` 的 [`Hal`]：恒等映射下返回的 `PhysAddr` 与可写虚拟指针相同。
struct VirtioMmioHal;

unsafe impl Hal for VirtioMmioHal {
    /// 按页数向帧池要连续物理页；失败时释放已拿页并返回空指针对，由上层映射为 [`DriverError`]。
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

/// VirtIO-MMIO 上的块设备（`virtio-blk`）。
pub struct VirtioBlkDevice {
    /// `virtio-drivers` 侧已握手的传输与队列状态。
    inner : Mutex<VirtIOBlk<VirtioMmioHal, MmioTransport<'static>>>,
    /// 是否已启用 IRQ 完成路径（由注册路径在 PLIC 接线成功后置位）。
    irq_mode : AtomicBool,
    /// 单请求在途门闩（原子）：等待任务睡眠时不持 `inner` 自旋锁，避免「持锁
    /// 跨 task 睡眠 + trap 唤醒」的锁序死锁；同一时刻只允许一个 IRQ 请求在途。
    in_flight : AtomicBool,
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
        Ok(Self { inner : Mutex::new(inner),
                  irq_mode : AtomicBool::new(false),
                  in_flight : AtomicBool::new(false) })
    }
}

impl BlockDevice for VirtioBlkDevice {
    fn total_blocks(&self) -> Option<u64> {
        Some(self.inner
                 .lock()
                 .capacity())
    }

    /// 以 LBA 为单位读入 `buf`；长度须为块大小的整数倍，否则由 VirtIO 层返回错误。
    fn read_blocks(&self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
        self.check_request_range(start_block, buf.len())?;
        if self.irq_mode.load(Ordering::Acquire) && runtime_ready() {
            self.read_blocks_irq(start_block, buf)
        } else {
            let start =
                usize::try_from(start_block.0).map_err(|_| DriverError::InvalidParam)?;
            let result = self.inner
                             .lock()
                             .read_blocks(start, buf)
                             .map_err(|_| DriverError::IoError);
            result
        }
    }

    /// 将 `buf` 写回磁盘；语义与 [`read_blocks`] 对称。
    fn write_blocks(&self, start_block : Lba, buf : &[u8]) -> DriverResult<()> {
        self.check_request_range(start_block, buf.len())?;
        if self.irq_mode.load(Ordering::Acquire) && runtime_ready() {
            self.write_blocks_irq(start_block, buf)
        } else {
            let start =
                usize::try_from(start_block.0).map_err(|_| DriverError::InvalidParam)?;
            let probe = buf.len() >= IOZONE_PROBE_MIN_WRITE_BYTES;
            if probe {
                logging::trace!("[virtio-blk-write] begin lba={} bytes={}",
                                start_block.0,
                                buf.len());
            }
            let result = self.inner
                             .lock()
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
    }

    fn flush(&self) -> DriverResult<()> {
        self.inner
            .lock()
            .flush()
            .map_err(|_| DriverError::IoError)
    }

    fn enable_irq(&self) {
        self.irq_mode.store(true, Ordering::Release);
    }
}

impl VirtioBlkDevice {
    fn read_blocks_irq(&self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
        // Linux 式：提交/等待/回收期间不持设备自旋锁；`in_flight` 只做单请求
        // 串行（多设备/多请求在途是后续 request_queue 任务）。
        while self.in_flight.swap(true, Ordering::AcqRel) {
            core::hint::spin_loop();
        }
        let result = self.read_blocks_irq_inner(start_block, buf);
        self.in_flight.store(false, Ordering::Release);
        result
    }

    fn read_blocks_irq_inner(&self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
        let mut req = BlkReq::default();
        let mut resp = BlkResp::default();
        IRQ_FIRED.store(false, Ordering::Release);
        let start = usize::try_from(start_block.0).map_err(|_| DriverError::InvalidParam)?;
        let token = {
            let mut inner = self.inner.lock();
            unsafe { inner.read_blocks_nb(start, &mut req, buf, &mut resp) }
                .map_err(|_| DriverError::IoError)?
        };
        loop {
            let done = {
                let mut inner = self.inner.lock();
                inner.peek_used() == Some(token)
            };
            if done {
                break;
            }
            IRQ_WAIT.call_once(|| WaitQueue::new_named("virtio-blk-irq"))
                    .wait_current_while(|| !consume_irq());
        }
        let mut inner = self.inner.lock();
        unsafe { inner.complete_read_blocks(token, &req, buf, &mut resp) }
            .map_err(|_| DriverError::IoError)
    }

    fn write_blocks_irq(&self, start_block : Lba, buf : &[u8]) -> DriverResult<()> {
        while self.in_flight.swap(true, Ordering::AcqRel) {
            core::hint::spin_loop();
        }
        let result = self.write_blocks_irq_inner(start_block, buf);
        self.in_flight.store(false, Ordering::Release);
        result
    }

    fn write_blocks_irq_inner(&self, start_block : Lba, buf : &[u8]) -> DriverResult<()> {
        let mut req = BlkReq::default();
        let mut resp = BlkResp::default();
        IRQ_FIRED.store(false, Ordering::Release);
        let start = usize::try_from(start_block.0).map_err(|_| DriverError::InvalidParam)?;
        let token = {
            let mut inner = self.inner.lock();
            unsafe { inner.write_blocks_nb(start, &mut req, buf, &mut resp) }
                .map_err(|_| DriverError::IoError)?
        };
        loop {
            let done = {
                let mut inner = self.inner.lock();
                inner.peek_used() == Some(token)
            };
            if done {
                break;
            }
            IRQ_WAIT.call_once(|| WaitQueue::new_named("virtio-blk-irq"))
                    .wait_current_while(|| !consume_irq());
        }
        let mut inner = self.inner.lock();
        unsafe { inner.complete_write_blocks(token, &req, buf, &mut resp) }
            .map_err(|_| DriverError::IoError)
    }
}
