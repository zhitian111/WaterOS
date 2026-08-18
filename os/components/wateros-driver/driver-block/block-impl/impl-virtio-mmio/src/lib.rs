//! VirtIO 块设备（MMIO 传输）实现，供平台驱动在枚举到 `virtio,mmio` + block 设备后实例化。
//!
//! **DMA / HAL**：`virtio-drivers` 的队列与内部缓冲通过 [`Hal::dma_alloc`] /
//! [`Hal::dma_dealloc`] 向本机要 **物理连续、页对齐、已清零** 的内存。此前使用固定 bump
//! 物理地址且 `dma_dealloc` 为空实现，易与内核其它内存及 VirtIO 传输重叠；现改为使用
//! 已初始化的全局 **帧分配器**（`wateros-mm-frame-alloctor`），与 Sv39 bring-up 一致。
//! 恒等映射下 `paddr == vaddr`（`usize`/`PhysAddr` 视图一致）。

#![no_std]
extern crate alloc;

use alloc::vec;
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

static RUNTIME_READY : AtomicBool = AtomicBool::new(false);

/// 平台全局中断与 PLIC 接线就绪后调用；此前 block IRQ 等待退化为同步路径。
pub fn enable_runtime_dispatch() {
    RUNTIME_READY.store(true, Ordering::Release);
}

fn runtime_ready() -> bool { RUNTIME_READY.load(Ordering::Acquire) }
/// 异步请求槽位数（队列深度上限；当前 FS 串行化下实际在途为 1，T07 锁拆分后
/// 才能并发填充多个槽位）。
const ASYNC_SLOTS : usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SlotKind { Read, Write }

#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotState { Free, InFlight, Done }

/// 单个在途请求：请求/响应/数据缓冲由驱动自持，避免中断上下文借用调用者栈引用。
struct PendingSlot {
    state : SlotState,
    kind : SlotKind,
    token : u16,
    req : BlkReq,
    resp : BlkResp,
    data : Vec<u8>,
    status_ok : bool,
}

impl PendingSlot {
    fn free() -> Self {
        Self { state : SlotState::Free,
               kind : SlotKind::Read,
               token : 0,
               req : BlkReq::default(),
               resp : BlkResp::default(),
               data : Vec::new(),
               status_ok : false }
    }
}

struct AsyncState {
    slots : Vec<PendingSlot>,
}

impl AsyncState {
    fn new() -> Self {
        Self { slots : (0..ASYNC_SLOTS).map(|_| PendingSlot::free()).collect() }
    }
}

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
    /// 异步请求槽位（submit/complete 路径共享）。
    async_state : Mutex<AsyncState>,
    /// 每槽位完成标志：供等待条件在调度器临界区内无锁复查（避免持调度器锁
    /// 期间去抢 `async_state` 导致与 bottom-half 成环死锁）。
    slots_done : [AtomicBool; ASYNC_SLOTS],
    /// 是否已启用 IRQ 完成路径（由注册路径在 PLIC 接线成功后置位）。
    irq_mode : AtomicBool,
    /// 请求完成等待队列：等待者睡眠，IRQ bottom-half 回收后唤醒。
    ///
    /// 驱动在 `task::init()` 之前的 boot 阶段实例化（`from_mmio`），而等待队列
    /// 依赖调度器注册表，因此必须惰性分配（首次等待/唤醒时 `task::init()` 已
    /// 完成），不能在构造器里创建。
    wait_queue : Once<WaitQueue>,
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
                  async_state : Mutex::new(AsyncState::new()),
                  slots_done : [const { AtomicBool::new(false) }; ASYNC_SLOTS],
                  irq_mode : AtomicBool::new(false),
                  wait_queue : Once::new() })
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
            self.inner
                .lock()
                .read_blocks(start, buf)
                .map_err(|_| DriverError::IoError)
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

    fn irq_bottom_half(&self) -> DriverResult<()> {
        {
            let mut state = self.async_state.lock();
            let mut inner = self.inner.lock();
            let _ = inner.ack_interrupt();
            loop {
                let Some(token) = inner.peek_used() else {
                    break;
                };
                let Some(slot_idx) = state.slots
                                          .iter()
                                          .position(|slot| slot.state == SlotState::InFlight &&
                                                            slot.token == token)
                else {
                    break;
                };
                let slot = &mut state.slots[slot_idx];
                let result = match slot.kind {
                    SlotKind::Read => unsafe {
                        inner.complete_read_blocks(token,
                                                   &slot.req,
                                                   &mut slot.data,
                                                   &mut slot.resp)
                    },
                    SlotKind::Write => unsafe {
                        inner.complete_write_blocks(token,
                                                    &slot.req,
                                                    &slot.data,
                                                    &mut slot.resp)
                    },
                };
                slot.state = SlotState::Done;
                slot.status_ok = result.is_ok();
                self.slots_done[slot_idx].store(true, Ordering::Release);
                logging::warn!("[virtio-blk-irq] bh complete slot={} token={} ok={}",
                               slot_idx,
                               token,
                               result.is_ok());
            }
        }
        // 锁已释放后唤醒等待者，避免 async_state → scheduler 锁序。等待队列
        // 尚未分配（boot 阶段自旋路径或 bottom-half 先于任何等待者运行）时
        // 跳过 wake：`wait_current_while` 会在调度器临界区内复查完成条件，
        // 不会丢失完成事件。
        if let Some(queue) = self.wait_queue.get() {
            let _ = queue.wake_all();
        }
        Ok(())
    }
}

impl VirtioBlkDevice {
    /// 提交异步请求并返回槽位下标；调用方随后自旋等待 [`Self::slot_done`]。
    fn submit_async(&self,
                    start_block : Lba,
                    buf : &[u8],
                    kind : SlotKind)
                    -> DriverResult<usize> {
        let start = usize::try_from(start_block.0).map_err(|_| DriverError::InvalidParam)?;
        // 槽位全忙时自旋等待（调用方持 FS 锁，不能睡眠；完成路径经 IRQ/bh 释放
        // 槽位）。自旋任务保持 runnable，可被定时器抢占，不构成持锁睡眠。
        let mut state = self.async_state.lock();
        let idx = loop {
            if let Some(idx) = state.slots
                                     .iter()
                                     .position(|slot| slot.state == SlotState::Free)
            {
                break idx;
            }
            drop(state);
            core::hint::spin_loop();
            state = self.async_state.lock();
        };
        self.slots_done[idx].store(false, Ordering::Release);
        {
            let slot = &mut state.slots[idx];
            slot.state = SlotState::InFlight;
            slot.kind = kind;
            slot.token = 0;
            slot.status_ok = false;
            slot.req = BlkReq::default();
            slot.resp = BlkResp::default();
            slot.data = match kind {
                SlotKind::Read => vec![0u8; buf.len()],
                SlotKind::Write => buf.to_vec(),
            };
        }
        let mut inner = self.inner.lock();
        let token = match kind {
            SlotKind::Read => {
                let slot = &mut state.slots[idx];
                unsafe { inner.read_blocks_nb(start, &mut slot.req, &mut slot.data, &mut slot.resp) }
            }
            SlotKind::Write => {
                let slot = &mut state.slots[idx];
                unsafe { inner.write_blocks_nb(start, &mut slot.req, &slot.data, &mut slot.resp) }
            }
        };
        match token {
            Ok(token) => {
                state.slots[idx].token = token;
                Ok(idx)
            }
            Err(_) => {
                state.slots[idx].state = SlotState::Free;
                state.slots[idx].data.clear();
                Err(DriverError::IoError)
            }
        }
    }

    fn slot_done(&self, idx : usize) -> bool {
        // 无锁读取：等待条件在调度器临界区内被复查，不能在此获取 Mutex。
        self.slots_done[idx].load(Ordering::Acquire)
    }

    /// 取回完成结果并把数据拷回调用方缓冲，随后释放槽位。
    fn finish_async(&self, idx : usize, buf : &mut [u8]) -> DriverResult<()> {
        let mut state = self.async_state.lock();
        let slot = &mut state.slots[idx];
        let status_ok = slot.status_ok;
        self.slots_done[idx].store(false, Ordering::Release);
        if slot.kind == SlotKind::Read {
            let n = buf.len().min(slot.data.len());
            buf[..n].copy_from_slice(&slot.data[..n]);
        }
        slot.state = SlotState::Free;
        slot.data.clear();
        if status_ok {
            Ok(())
        } else {
            Err(DriverError::IoError)
        }
    }

    fn read_blocks_irq(&self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
        let slot = self.submit_async(start_block, buf, SlotKind::Read)?;
        self.wait_irq_completion(slot);
        self.finish_async(slot, buf)
    }

    fn write_blocks_irq(&self, start_block : Lba, buf : &[u8]) -> DriverResult<()> {
        let slot = self.submit_async(start_block, buf, SlotKind::Write)?;
        self.wait_irq_completion(slot);
        self.finish_async(slot, &mut [])
    }

    /// 任务睡眠等待 IRQ 完成：top-half ack + bottom-half 回收 used ring 并唤醒
    /// 本队列；2 tick 超时兜底偶发唤醒丢失，超时后直接 drain 一次自愈（与旧
    /// 自旋路径的定期回收语义一致），仍未完成则继续挂起等待。
    ///
    /// boot 阶段（`run_first_task` 之前）当前上下文只是调度器的 idle 占位，
    /// 不能经 waitqueue 阻塞；退化为 T06 已验证的自旋 + 定期 drain 路径。
    fn wait_irq_completion(&self, slot : usize) {
        self.wait_queue
            .call_once(|| WaitQueue::new_named("virtio-blk-irq"))
            .wait_current_while(|| !self.slot_done(slot));
    }
}
