//! 全局栈式物理帧分配器（自旋锁保护）：`FrameId` 为
//! **PPN**，与 4 KiB 物理页一一对应。
//!
//! 调用方须保证 `init_frame_allocator` 传入的 PPN 区间落在
//! **内核可安全分配/回收** 的 RAM 内；本模块不校验与页表或设备的重叠。

#![no_std]
#![allow(static_mut_refs)]
extern crate alloc;

use alloc::vec::Vec;

use api_v0::{FrameAllocError, FrameAllocResult, FrameMemStats, PhysicalFrameAllocator};
use mm_api::addr::{PhysPageNum, PAGE_SIZE};
use wateros_base::sync::MultiprocessorSafeCell;
use arch::interrupt::{
    disable_global_interrupt, read_global_interrupt_state, restore_global_interrupt_state,
    ArchInterruptState,
};

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

const ZEROED_POOL_CAPACITY : usize = 1024;
const ZEROED_POOL_LOW_WATERMARK : usize = 256;
const ZEROED_POOL_HIGH_WATERMARK : usize = ZEROED_POOL_CAPACITY;
const ZEROED_SYNC_BATCH : usize = 16;
const ZEROED_IDLE_BATCH : usize = 32;
const ZEROED_OOM_DRAIN_BATCH : usize = 32;

#[cfg(feature = "zeroed-frame-pool-stats")]
macro_rules! pool_stats {
    ($($body:tt)*) => { $($body)* };
}

#[cfg(not(feature = "zeroed-frame-pool-stats"))]
macro_rules! pool_stats {
    ($($body:tt)*) => {};
}

struct FrameAllocatorInterruptGuard {
    state : ArchInterruptState,
}

impl FrameAllocatorInterruptGuard {
    fn new() -> Self {
        let state = read_global_interrupt_state()
            .expect("read global interrupt state for frame allocator guard");
        disable_global_interrupt().expect("disable global interrupt for frame allocator guard");
        Self { state }
    }
}

impl Drop for FrameAllocatorInterruptGuard {
    fn drop(&mut self) {
        restore_global_interrupt_state(self.state)
            .expect("restore global interrupt state for frame allocator guard");
    }
}

fn with_frame_allocator<R>(f : impl FnOnce(&mut StackFrameAllocator) -> R) -> R {
    let _irq = FrameAllocatorInterruptGuard::new();
    let cell = get_frame_allocator_cell();
    let cpu = arch::cpu::current_cpu_id().raw();
    let object = cell as *const _ as usize;
    let mut allocator = if debug::ENABLED {
        if let Some(guard) = cell.try_lock() {
            guard
        } else {
            debug::lock_wait(cpu,
                             0,
                             debug::NO_TASK,
                             debug::DebugLockKind::FrameAllocator,
                             object);
            cell.exclusive_access()
        }
    } else {
        cell.exclusive_access()
    };
    debug::lock_acquired(cpu, debug::DebugLockKind::FrameAllocator, object);
    let result = f(&mut allocator);
    drop(allocator);
    debug::lock_released(cpu, debug::DebugLockKind::FrameAllocator, object);
    result
}

/// Idle 维护不能为了拿 allocator 锁而自旋；锁忙时本轮直接进入 WFI。
fn try_with_frame_allocator<R>(f : impl FnOnce(&mut StackFrameAllocator) -> R) -> Option<R> {
    let _irq = FrameAllocatorInterruptGuard::new();
    let cell = get_frame_allocator_cell();
    let cpu = arch::cpu::current_cpu_id().raw();
    let object = cell as *const _ as usize;
    let mut allocator = cell.try_lock()?;
    debug::lock_acquired(cpu, debug::DebugLockKind::FrameAllocator, object);
    let result = f(&mut allocator);
    drop(allocator);
    debug::lock_released(cpu, debug::DebugLockKind::FrameAllocator, object);
    Some(result)
}

/// 临时的 LIFO 栈式帧分配器：
/// - `init(start_ppn, end_ppn)` 产生 free-list；
/// - `alloc_frame()` 从栈顶 pop；
/// - `dealloc_frame()` push 回栈（顺序回放）。
///
/// 注意：该实现未做“重复释放/未分配校验”，属于早期阶段可接受的简化。
///
/// 空闲帧来自两段语义：尚未动过的连续高段 `[start_ppn,
/// next_novel)`（惰性下推）， 以及显式回收栈 `recycled`。不在 `init` 时把整段
/// PPN 推入 `Vec`，避免大内存下撑爆内核 heap。
pub struct StackFrameAllocator {
    recycled : Vec<PhysPageNum>,
    allocated : Vec<bool>,
    /// 每帧共享引用计数。`u32` 已远高于实际单页映射数量，同时避免大内存机器
    /// 为每个尚未使用的物理页消耗一个 64 位字。
    ref_counts : Vec<u32>,
    start_ppn : usize,
    end_ppn : usize,
    /// 永不分配的内存内保留区间（如引导 DTB），已裁剪到帧池范围。
    reserved_start_ppn : usize,
    reserved_end_ppn : usize,
    /// 仍可从连续区分配的第一页号上界（不包含）；初始为 `end_ppn`。
    next_novel : usize,
}

impl StackFrameAllocator {
    /// 构造空分配器；须再调用 [`Self::init`] 方可从 PPN 区间取帧。
    pub fn new() -> Self {
        Self { recycled : Vec::new(),
               allocated : Vec::new(),
               ref_counts : Vec::new(),
               start_ppn : 0,
               end_ppn : 0,
               reserved_start_ppn : 0,
               reserved_end_ppn : 0,
               next_novel : 0 }
    }

    /// 将可用帧限制为半开区间 `[start_ppn,
    /// end_ppn)`（PPN）；会清空回收栈并重置惰性上界。
    pub fn init(&mut self, start_ppn : PhysPageNum, end_ppn : PhysPageNum) {
        self.init_with_reserved(start_ppn, end_ppn, start_ppn, start_ppn);
    }

    /// 初始化完整 RAM 帧区间，同时排除一个位于其中的半开保留区间。
    ///
    /// 保留区不从 novel/recycled 路径返回，也不能被引用计数或回收接口接受。
    pub fn init_with_reserved(&mut self,
                              start_ppn : PhysPageNum,
                              end_ppn : PhysPageNum,
                              reserved_start_ppn : PhysPageNum,
                              reserved_end_ppn : PhysPageNum) {
        self.recycled
            .clear();
        self.start_ppn = start_ppn.0;
        self.end_ppn = end_ppn.0;
        self.reserved_start_ppn = reserved_start_ppn.0.clamp(self.start_ppn, self.end_ppn);
        self.reserved_end_ppn = reserved_end_ppn.0.clamp(self.reserved_start_ppn,
                                                         self.end_ppn);
        self.next_novel = end_ppn.0;
        self.allocated
            .clear();
        self.allocated
            .resize(end_ppn.0.saturating_sub(start_ppn.0), false);
        self.ref_counts
            .clear();
        self.ref_counts
            .resize(end_ppn.0.saturating_sub(start_ppn.0), 0);
    }

    #[inline]
    fn index(&self, frame : PhysPageNum) -> Option<usize> {
        if frame.0 < self.start_ppn ||
           frame.0 >= self.end_ppn ||
           self.is_reserved(frame.0)
        {
            None
        } else {
            Some(frame.0 - self.start_ppn)
        }
    }

    #[inline]
    fn is_reserved(&self, ppn : usize) -> bool {
        self.reserved_start_ppn <= ppn && ppn < self.reserved_end_ppn
    }

    fn novel_free_frames(&self) -> usize {
        let total = self.next_novel.saturating_sub(self.start_ppn);
        let reserved_below_next = self.next_novel
                                      .min(self.reserved_end_ppn)
                                      .saturating_sub(self.reserved_start_ppn);
        total.saturating_sub(reserved_below_next)
    }

    /// 只读内存池统计：总帧 = 区间大小；空闲 = 回收栈 + 未分配连续段。
    pub fn mem_stats(&self) -> FrameMemStats {
        let reserved_frames = self.reserved_end_ppn.saturating_sub(self.reserved_start_ppn);
        let total_frames = self.end_ppn
                               .saturating_sub(self.start_ppn)
                               .saturating_sub(reserved_frames);
        let novel_free = self.novel_free_frames();
        let free_frames = self.recycled.len().saturating_add(novel_free);
        FrameMemStats {
            total_frames,
            free_frames: free_frames.min(total_frames),
            page_bytes: PAGE_SIZE,
        }
    }

    /// 在一次 allocator 临界区内取得至多 `out.len()` 个 raw frame。
    fn alloc_batch(&mut self, out : &mut [PhysPageNum]) -> FrameAllocResult<usize> {
        let mut count = 0usize;
        while count < out.len() {
            match self.alloc_frame() {
                Ok(frame) => {
                    out[count] = frame;
                    count += 1;
                }
                Err(FrameAllocError::OutOfMemory) if count != 0 => break,
                Err(error) => {
                    for frame in out[..count].iter().copied() {
                        let _ = self.dealloc_frame(frame);
                    }
                    return Err(error);
                }
            }
        }
        if count == 0 {
            Err(FrameAllocError::OutOfMemory)
        } else {
            Ok(count)
        }
    }
}

impl PhysicalFrameAllocator for StackFrameAllocator {
    type FrameId = PhysPageNum;

    fn alloc_frame(&mut self) -> FrameAllocResult<Self::FrameId> {
        while let Some(p) = self.recycled.pop() {
            let Some(idx) = self.index(p) else {
                log::warn!("[frame-allocator] drop invalid recycled ppn={:#x} range=[{:#x},{:#x})",
                           p.0,
                           self.start_ppn,
                           self.end_ppn);
                continue;
            };
            if self.allocated[idx] {
                log::warn!("[frame-allocator] drop duplicate recycled ppn={:#x}",
                           p.0);
                continue;
            }
            self.allocated[idx] = true;
            self.ref_counts[idx] = 1;
            return Ok(p);
        }
        if self.next_novel == self.reserved_end_ppn {
            self.next_novel = self.reserved_start_ppn;
        }
        if self.next_novel > self.start_ppn {
            self.next_novel -= 1;
            let p = PhysPageNum(self.next_novel);
            let idx = self.next_novel - self.start_ppn;
            if self.allocated[idx] {
                log::warn!("[frame-allocator] novel ppn already allocated ppn={:#x}",
                           p.0);
                return Err(FrameAllocError::InvalidFrame);
            }
            self.allocated[idx] = true;
            self.ref_counts[idx] = 1;
            return Ok(p);
        }
        Err(FrameAllocError::OutOfMemory)
    }

    fn dealloc_frame(&mut self, frame : Self::FrameId) -> FrameAllocResult<()> {
        let Some(idx) = self.index(frame) else {
            log::warn!("[frame-allocator] invalid dealloc ppn={:#x} range=[{:#x},{:#x})",
                       frame.0,
                       self.start_ppn,
                       self.end_ppn);
            return Err(FrameAllocError::InvalidFrame);
        };
        if frame.0 < self.next_novel || !self.allocated[idx] || self.ref_counts[idx] == 0 {
            log::warn!("[frame-allocator] invalid dealloc ppn={:#x} next_novel={:#x} \
                        allocated={} ref_count={}",
                       frame.0,
                       self.next_novel,
                       self.allocated[idx],
                       self.ref_counts[idx]);
            return Err(FrameAllocError::InvalidFrame);
        }
        if self.ref_counts[idx] > 1 {
            self.ref_counts[idx] -= 1;
            return Ok(());
        }
        self.ref_counts[idx] = 0;
        self.allocated[idx] = false;
        self.recycled
            .push(frame);
        Ok(())
    }
}

impl StackFrameAllocator {
    pub fn inc_ref(&mut self, frame : PhysPageNum) -> FrameAllocResult<usize> {
        let Some(idx) = self.index(frame) else {
            log::warn!("[frame-allocator] invalid inc_ref ppn={:#x} range=[{:#x},{:#x})",
                       frame.0,
                       self.start_ppn,
                       self.end_ppn);
            return Err(FrameAllocError::InvalidFrame);
        };
        if !self.allocated[idx] || self.ref_counts[idx] == 0 {
            log::warn!("[frame-allocator] inc_ref on unallocated ppn={:#x} allocated={} \
                        ref_count={}",
                       frame.0,
                       self.allocated[idx],
                       self.ref_counts[idx]);
            return Err(FrameAllocError::InvalidFrame);
        }
        self.ref_counts[idx] = self.ref_counts[idx].saturating_add(1);
        Ok(self.ref_counts[idx] as usize)
    }

    pub fn ref_count(&self, frame : PhysPageNum) -> FrameAllocResult<usize> {
        let Some(idx) = self.index(frame) else {
            return Err(FrameAllocError::InvalidFrame);
        };
        if !self.allocated[idx] || self.ref_counts[idx] == 0 {
            return Err(FrameAllocError::InvalidFrame);
        }
        Ok(self.ref_counts[idx] as usize)
    }
}

// ===== 全局单例（BSP 初始化，运行期多核加锁）=====

static mut FRAME_ALLOCATOR : MaybeUninit<MultiprocessorSafeCell<StackFrameAllocator>> =
    MaybeUninit::uninit();
static FRAME_ALLOCATOR_READY : AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Default)]
pub struct ZeroedFramePoolStats {
    pub demand_hits : u64,
    pub demand_misses : u64,
    pub prefault_hits : u64,
    pub prefault_misses : u64,
    pub sync_refill_batches : u64,
    pub sync_refill_pages : u64,
    pub idle_refill_batches : u64,
    pub idle_refill_pages : u64,
    pub idle_lock_busy : u64,
    pub low_watermark_activations : u64,
    pub raw_oom_drains : u64,
    pub raw_oom_drain_pages : u64,
    pub peak_len : usize,
    pub current_len : usize,
    pub in_flight : usize,
    pub capacity : usize,
    pub low_watermark : usize,
}

impl ZeroedFramePoolStats {
    const fn empty() -> Self {
        Self { demand_hits : 0,
               demand_misses : 0,
               prefault_hits : 0,
               prefault_misses : 0,
               sync_refill_batches : 0,
               sync_refill_pages : 0,
               idle_refill_batches : 0,
               idle_refill_pages : 0,
               idle_lock_busy : 0,
               low_watermark_activations : 0,
               raw_oom_drains : 0,
               raw_oom_drain_pages : 0,
               peak_len : 0,
               current_len : 0,
               in_flight : 0,
               capacity : ZEROED_POOL_CAPACITY,
               low_watermark : ZEROED_POOL_LOW_WATERMARK }
    }
}

#[derive(Clone, Copy)]
enum ZeroedTakeKind {
    Demand,
    Prefault,
    Retry,
}

#[derive(Clone, Copy)]
enum ZeroedRefillKind {
    Sync,
    Idle,
}

/// 池中和正在清零的 frame 都保持 allocator 的 `allocated=true, ref_count=1`。
/// `in_flight` 是生产者的发布槽预留，防止多个 idle CPU 同时越过容量。
struct ZeroedFramePool {
    frames : [PhysPageNum; ZEROED_POOL_CAPACITY],
    len : usize,
    in_flight : usize,
    refill_active : bool,
    stats : ZeroedFramePoolStats,
}

impl ZeroedFramePool {
    const fn new() -> Self {
        Self { frames : [PhysPageNum(0); ZEROED_POOL_CAPACITY],
               len : 0,
               in_flight : 0,
               refill_active : true,
               stats : ZeroedFramePoolStats::empty() }
    }

    fn reset(&mut self) {
        self.len = 0;
        self.in_flight = 0;
        self.refill_active = true;
        self.reset_stats();
    }

    fn reset_stats(&mut self) {
        self.stats = ZeroedFramePoolStats { peak_len : self.len,
                                            current_len : self.len,
                                            in_flight : self.in_flight,
                                            capacity : ZEROED_POOL_CAPACITY,
                                            low_watermark : ZEROED_POOL_LOW_WATERMARK,
                                            ..ZeroedFramePoolStats::default() };
    }

    fn update_refill_state(&mut self) {
        let effective = self.len.saturating_add(self.in_flight);
        if effective < ZEROED_POOL_LOW_WATERMARK {
            if !self.refill_active {
                pool_stats! { self.stats.low_watermark_activations += 1; }
            }
            self.refill_active = true;
        } else if effective >= ZEROED_POOL_HIGH_WATERMARK {
            self.refill_active = false;
        }
    }

    fn take(&mut self,
            preserve_low_watermark : bool,
            kind : ZeroedTakeKind)
            -> Option<PhysPageNum> {
        if self.len == 0 || (preserve_low_watermark && self.len <= ZEROED_POOL_LOW_WATERMARK) {
            match kind {
                ZeroedTakeKind::Demand => {
                    pool_stats! { self.stats.demand_misses += 1; }
                }
                ZeroedTakeKind::Prefault => {
                    pool_stats! { self.stats.prefault_misses += 1; }
                }
                ZeroedTakeKind::Retry => {}
            }
            return None;
        }
        self.len -= 1;
        let frame = self.frames[self.len];
        match kind {
            ZeroedTakeKind::Demand => {
                pool_stats! { self.stats.demand_hits += 1; }
            }
            ZeroedTakeKind::Prefault => {
                pool_stats! { self.stats.prefault_hits += 1; }
            }
            ZeroedTakeKind::Retry => {}
        }
        self.update_refill_state();
        Some(frame)
    }

    fn claim_sync_publish_slots(&mut self, wanted : usize) -> usize {
        let available = ZEROED_POOL_CAPACITY.saturating_sub(self.len.saturating_add(self.in_flight));
        let claimed = wanted.min(available);
        self.in_flight += claimed;
        claimed
    }

    fn claim_idle_publish_slots(&mut self) -> usize {
        self.update_refill_state();
        if !self.refill_active {
            return 0;
        }
        let effective = self.len.saturating_add(self.in_flight);
        let claimed = ZEROED_IDLE_BATCH.min(ZEROED_POOL_HIGH_WATERMARK.saturating_sub(effective));
        self.in_flight += claimed;
        self.update_refill_state();
        claimed
    }

    fn finish_claim(&mut self,
                    claimed : usize,
                    frames : &[PhysPageNum],
                    kind : ZeroedRefillKind,
                    produced_pages : usize) {
        debug_assert!(frames.len() <= claimed);
        debug_assert!(claimed <= self.in_flight);
        debug_assert!(self.len.saturating_add(frames.len()) <= ZEROED_POOL_CAPACITY);
        for frame in frames.iter().copied() {
            self.frames[self.len] = frame;
            self.len += 1;
        }
        self.in_flight -= claimed;
        if produced_pages != 0 {
            match kind {
                ZeroedRefillKind::Sync => {
                    pool_stats! {
                        self.stats.sync_refill_batches += 1;
                        self.stats.sync_refill_pages += produced_pages as u64;
                    }
                }
                ZeroedRefillKind::Idle => {
                    pool_stats! {
                        self.stats.idle_refill_batches += 1;
                        self.stats.idle_refill_pages += produced_pages as u64;
                    }
                }
            }
        }
        pool_stats! { self.stats.peak_len = self.stats.peak_len.max(self.len); }
        self.update_refill_state();
    }

    fn drain(&mut self, out : &mut [PhysPageNum]) -> usize {
        let count = self.len.min(out.len());
        let start = self.len - count;
        out[..count].copy_from_slice(&self.frames[start..self.len]);
        self.len = start;
        if count != 0 {
            pool_stats! {
                self.stats.raw_oom_drains += 1;
                self.stats.raw_oom_drain_pages += count as u64;
            }
        }
        self.update_refill_state();
        count
    }

    fn stats(&self) -> ZeroedFramePoolStats {
        ZeroedFramePoolStats { current_len : self.len,
                               in_flight : self.in_flight,
                               ..self.stats }
    }
}

static ZEROED_FRAME_POOL : MultiprocessorSafeCell<ZeroedFramePool> =
    MultiprocessorSafeCell::new(ZeroedFramePool::new());

fn with_zeroed_frame_pool<R>(f : impl FnOnce(&mut ZeroedFramePool) -> R) -> R {
    let _irq = FrameAllocatorInterruptGuard::new();
    let mut pool = ZEROED_FRAME_POOL.exclusive_access();
    f(&mut pool)
}

#[inline]
fn zero_frame(frame : PhysPageNum) {
    unsafe {
        core::ptr::write_bytes((frame.0 * PAGE_SIZE) as *mut u8, 0, PAGE_SIZE);
    }
}

// BOOT_CONTRACT: `FRAME_ALLOCATOR` 只能由 BSP 在开放 AP 前初始化。`READY` 的
// Release/Acquire 负责发布已构造对象，不把并发的首次初始化变成安全操作。
fn get_frame_allocator_cell() -> &'static MultiprocessorSafeCell<StackFrameAllocator> {
    assert!(FRAME_ALLOCATOR_READY.load(Ordering::Acquire),
            "frame allocator not initialized: call init_frame_allocator() first");
    unsafe { &*FRAME_ALLOCATOR.as_ptr() }
}

/// 初始化全局帧分配器（临时 stack 实现）。
pub fn init_frame_allocator(start_ppn : PhysPageNum, end_ppn : PhysPageNum) {
    init_frame_allocator_with_reserved(start_ppn, end_ppn, start_ppn, start_ppn);
}

/// 初始化完整物理帧区间，并排除一个半开保留区间。
pub fn init_frame_allocator_with_reserved(start_ppn : PhysPageNum,
                                          end_ppn : PhysPageNum,
                                          reserved_start_ppn : PhysPageNum,
                                          reserved_end_ppn : PhysPageNum) {
    if !FRAME_ALLOCATOR_READY.load(Ordering::Acquire) {
        unsafe {
            FRAME_ALLOCATOR.write(MultiprocessorSafeCell::new(StackFrameAllocator::new()));
        }
        FRAME_ALLOCATOR_READY.store(true, Ordering::Release);
    }
    // 该入口也供 allocator 自测重置状态；旧池帧随后会被 allocator 的 init
    // 一并重新标记为空闲，不能把旧 PPN 留在池中。
    with_zeroed_frame_pool(ZeroedFramePool::reset);
    get_frame_allocator_cell();
    with_frame_allocator(|allocator| {
        allocator.init_with_reserved(start_ppn,
                                     end_ppn,
                                     reserved_start_ppn,
                                     reserved_end_ppn)
    });
}

/// 获取全局帧分配器单例容器（供特殊场景直接拿 `exclusive_access()`）。
pub fn frame_allocator_cell() -> &'static MultiprocessorSafeCell<StackFrameAllocator> {
    get_frame_allocator_cell()
}

/// 分配一个物理帧（返回帧标识）。
pub fn frame_alloc() -> Option<PhysPageNum> {
    frame_alloc_result().ok()
}

/// 回收一个物理帧。
pub fn frame_dealloc(frame : PhysPageNum) {
    if let Err(err) = with_frame_allocator(|allocator| allocator.dealloc_frame(frame)) {
        log::warn!("[frame-allocator] ignored dealloc ppn={:#x}: {:?}",
                   frame.0,
                   err);
    }
}

pub fn frame_alloc_result() -> FrameAllocResult<PhysPageNum> {
    let first = with_frame_allocator(|allocator| allocator.alloc_frame());
    if first != Err(FrameAllocError::OutOfMemory) {
        return first;
    }

    // 预清零池是可回收缓存。raw 分配耗尽时先在独立临界区摘下池中页，再批量
    // 归还 allocator，绝不同时持有两把锁。
    let mut reclaimed = [PhysPageNum(0); ZEROED_OOM_DRAIN_BATCH];
    let count = with_zeroed_frame_pool(|pool| pool.drain(&mut reclaimed));
    if count == 0 {
        return first;
    }
    with_frame_allocator(|allocator| {
        for frame in reclaimed[..count].iter().copied() {
            allocator.dealloc_frame(frame)?;
        }
        allocator.alloc_frame()
    })
}

/// 分配一页已清零 frame。池 miss 时当前 CPU 一次锁内批量取得 raw frame，
/// 锁外清零，返回一页并发布其余页。
pub fn frame_alloc_zeroed_result() -> FrameAllocResult<PhysPageNum> {
    if let Some(frame) =
        with_zeroed_frame_pool(|pool| pool.take(false, ZeroedTakeKind::Demand))
    {
        return Ok(frame);
    }

    let claimed = with_zeroed_frame_pool(|pool| {
        pool.claim_sync_publish_slots(ZEROED_SYNC_BATCH - 1)
    });
    let wanted = 1 + claimed;
    let mut frames = [PhysPageNum(0); ZEROED_SYNC_BATCH];
    let allocated = with_frame_allocator(|allocator| allocator.alloc_batch(&mut frames[..wanted]));
    let count = match allocated {
        Ok(count) => count,
        Err(error) => {
            with_zeroed_frame_pool(|pool| {
                pool.finish_claim(claimed, &[], ZeroedRefillKind::Sync, 0)
            });
            // 一个并发 idle 生产者可能刚好在 raw 分配失败后发布了页。
            return with_zeroed_frame_pool(|pool| pool.take(false, ZeroedTakeKind::Retry))
                .ok_or(error);
        }
    };

    for frame in frames[..count].iter().copied() {
        zero_frame(frame);
    }
    let publish_count = count.saturating_sub(1);
    with_zeroed_frame_pool(|pool| {
        pool.finish_claim(claimed,
                          &frames[1..1 + publish_count],
                          ZeroedRefillKind::Sync,
                          count);
    });
    Ok(frames[0])
}

/// 只在池高于低水位时取一页，不分配、不清零。ELF BSS 预映射用它保证
/// `exec` 前台只消费 idle CPU 已完成的工作；返回 `None` 时继续保留 lazy 映射。
pub fn try_alloc_zeroed_frame_for_prefault() -> Option<PhysPageNum> {
    with_zeroed_frame_pool(|pool| pool.take(true, ZeroedTakeKind::Prefault))
}

/// Idle task 在每次 WFI 前执行一轮有界维护。allocator 锁忙时立即放弃本轮；
/// 成功取得的 raw frame 在所有锁外清零，再用一次短临界区发布 PPN。
pub fn idle_zeroed_frame_pool_maintenance() {
    if !FRAME_ALLOCATOR_READY.load(Ordering::Acquire) {
        return;
    }
    let claimed = with_zeroed_frame_pool(ZeroedFramePool::claim_idle_publish_slots);
    if claimed == 0 {
        return;
    }

    let mut frames = [PhysPageNum(0); ZEROED_IDLE_BATCH];
    let Some(allocated) = try_with_frame_allocator(|allocator| {
        allocator.alloc_batch(&mut frames[..claimed])
    }) else {
        with_zeroed_frame_pool(|pool| {
            pool_stats! { pool.stats.idle_lock_busy += 1; }
            pool.finish_claim(claimed, &[], ZeroedRefillKind::Idle, 0);
        });
        return;
    };
    let count = match allocated {
        Ok(count) => count,
        Err(_) => {
            with_zeroed_frame_pool(|pool| {
                pool.finish_claim(claimed, &[], ZeroedRefillKind::Idle, 0)
            });
            return;
        }
    };
    for frame in frames[..count].iter().copied() {
        zero_frame(frame);
    }
    with_zeroed_frame_pool(|pool| {
        pool.finish_claim(claimed, &frames[..count], ZeroedRefillKind::Idle, count)
    });
}

pub fn reset_zeroed_frame_pool_stats() {
    with_zeroed_frame_pool(ZeroedFramePool::reset_stats);
}

pub fn zeroed_frame_pool_stats() -> ZeroedFramePoolStats {
    with_zeroed_frame_pool(|pool| pool.stats())
}

pub fn log_zeroed_frame_pool_stats(label : &str) {
    let stats = zeroed_frame_pool_stats();
    log::error!("[frame-zeroed-pool] label={} demand_hit={} demand_miss={} prefault_hit={} \
                 prefault_miss={} sync_batches={} sync_pages={} idle_batches={} idle_pages={} \
                 idle_lock_busy={} low_activations={} oom_drains={} oom_drain_pages={} \
                 peak_len={} current_len={} in_flight={} capacity={} low={}",
                label,
                stats.demand_hits,
                stats.demand_misses,
                stats.prefault_hits,
                stats.prefault_misses,
                stats.sync_refill_batches,
                stats.sync_refill_pages,
                stats.idle_refill_batches,
                stats.idle_refill_pages,
                stats.idle_lock_busy,
                stats.low_watermark_activations,
                stats.raw_oom_drains,
                stats.raw_oom_drain_pages,
                stats.peak_len,
                stats.current_len,
                stats.in_flight,
                stats.capacity,
                stats.low_watermark);
}

pub fn frame_dealloc_result(frame : PhysPageNum) -> FrameAllocResult<()> {
    with_frame_allocator(|allocator| allocator.dealloc_frame(frame))
}

pub fn frame_inc_ref(frame : PhysPageNum) -> FrameAllocResult<usize> {
    with_frame_allocator(|allocator| allocator.inc_ref(frame))
}

pub fn frame_ref_count(frame : PhysPageNum) -> FrameAllocResult<usize> {
    with_frame_allocator(|allocator| allocator.ref_count(frame))
}

/// 全局帧池只读统计；未初始化时返回零值。
pub fn frame_mem_stats() -> FrameMemStats {
    if !FRAME_ALLOCATOR_READY.load(Ordering::Acquire) {
        return FrameMemStats {
            page_bytes: PAGE_SIZE,
            ..FrameMemStats::default()
        };
    }
    let mut stats = with_frame_allocator(|allocator| allocator.mem_stats());
    let zeroed_free = with_zeroed_frame_pool(|pool| pool.len);
    stats.free_frames = stats.free_frames
                             .saturating_add(zeroed_free)
                             .min(stats.total_frames);
    stats
}

/// 零大小适配器：实现 [`PhysicalFrameAllocator`] 时每次调用短借全局栈式分配器。
///
/// 供 `HeapBrk` / `MmapOps` 等路径使用。**不得**在已持有
/// [`frame_allocator_cell`] 的 [`MultiprocessorSafeCell::exclusive_access`]
/// 期间再跑会嵌套调用 [`frame_alloc_result`] 的页表 walk，否则重入同一
/// 自旋锁会永久等待。
pub struct GlobalPhysFrameAllocator;

impl PhysicalFrameAllocator for GlobalPhysFrameAllocator {
    type FrameId = PhysPageNum;

    #[inline]
    fn alloc_frame(&mut self) -> FrameAllocResult<Self::FrameId> { frame_alloc_result() }

    #[inline]
    fn try_alloc_zeroed_frame(&mut self) -> FrameAllocResult<Option<Self::FrameId>> {
        frame_alloc_zeroed_result().map(Some)
    }

    #[inline]
    fn dealloc_frame(&mut self, frame : Self::FrameId) -> FrameAllocResult<()> {
        frame_dealloc_result(frame)
    }
}

pub fn test_with_range(start_ppn : PhysPageNum, end_ppn : PhysPageNum) {
    log::trace!("[frame-alloctor::impl-stack] test begin");
    log::trace!("[frame-alloctor::impl-stack] init range: [{:#x}, {:#x})",
                start_ppn.0,
                end_ppn.0);

    init_frame_allocator(start_ppn, end_ppn);

    let cap = end_ppn.0
                     .saturating_sub(start_ppn.0);
    if cap == 0 {
        log::info!("[frame-alloctor::impl-stack] empty range, skip alloc test");
        log::trace!("[frame-alloctor::impl-stack] test end");
        return;
    }

    // 大内存下不要用单个 Vec 持有全部帧（元数据会占满内核 heap）。
    const BATCH : usize = 256;
    let mut buf : Vec<PhysPageNum> = Vec::new();
    let mut done = 0usize;
    while done < cap {
        let n = BATCH.min(cap - done);
        for _ in 0..n {
            buf.push(frame_alloc_result().expect("alloc in batch"));
            done += 1;
        }
        for f in buf.drain(..) {
            frame_dealloc_result(f).expect("dealloc in batch");
        }
    }

    // 池应已复原；再取一页并归还
    let one = frame_alloc_result().expect("alloc after treadmill");
    frame_dealloc_result(one).expect("dealloc after treadmill");

    // 仅在小池上验证「耗尽后 OOM」（需同时持有 cap 个句柄，heap 可承受）
    const MAX_FULL_HOLD : usize = 8192;
    if cap <= MAX_FULL_HOLD {
        let mut frames : Vec<PhysPageNum> = Vec::new();
        for _ in 0..cap {
            frames.push(frame_alloc_result().expect("alloc before OOM"));
        }
        assert!(frame_alloc_result().is_err(),
                "should be OOM after exhausting frames");
        for f in frames.drain(..) {
            frame_dealloc_result(f).expect("dealloc should succeed");
        }
        let _ = frame_alloc_result().expect("alloc should succeed after recycle");
    } else {
        log::trace!("[frame-alloctor::impl-stack] skip full OOM witness ({} pages > {})",
                    cap,
                    MAX_FULL_HOLD);
    }

    // Leave the global allocator in the same pristine state expected by bring-up callers.
    init_frame_allocator(start_ppn, end_ppn);

    log::trace!("[frame-alloctor::impl-stack] test end");
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[frame-allocator/impl-stack] self_test begin");
    assert!(core::mem::size_of::<FrameMemStats>() > 0);
    log::info!("[frame-allocator/impl-stack] self_test complete");
}
