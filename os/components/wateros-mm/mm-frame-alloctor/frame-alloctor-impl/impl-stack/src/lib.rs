//! 全局栈式物理帧分配器（自旋锁保护）：`FrameId` 为
//! **PPN**，与 4 KiB 物理页一一对应。
//!
//! 调用方须保证 `init_frame_allocator` 传入的 PPN 区间落在
//! **内核可安全分配/回收** 的 RAM 内；本模块不校验与页表或设备的重叠。

#![no_std]
#![allow(static_mut_refs)]
extern crate alloc;

use alloc::vec::Vec;

use api_v0::{FrameAllocError, FrameAllocResult, FrameMemStats, FrameSpan, PhysicalFrameAllocator};
#[cfg(feature = "kernel-arch")]
use arch::interrupt::{
    disable_global_interrupt, read_global_interrupt_state, restore_global_interrupt_state,
    ArchInterruptState,
};
use mm_api::addr::{PhysPageNum, PAGE_SIZE};
use wateros_base::sync::MultiprocessorSafeCell;

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

struct FrameAllocatorInterruptGuard {
    #[cfg(feature = "kernel-arch")]
    state : ArchInterruptState,
}

impl FrameAllocatorInterruptGuard {
    fn new() -> Self {
        #[cfg(feature = "kernel-arch")]
        let state = read_global_interrupt_state().expect("read global interrupt state for frame \
                                                          allocator guard");
        #[cfg(feature = "kernel-arch")]
        disable_global_interrupt().expect("disable global interrupt for frame allocator guard");
        Self { #[cfg(feature = "kernel-arch")]
               state }
    }
}

impl Drop for FrameAllocatorInterruptGuard {
    fn drop(&mut self) {
        #[cfg(feature = "kernel-arch")]
        restore_global_interrupt_state(self.state).expect("restore global interrupt state for \
                                                           frame allocator guard");
    }
}

fn with_frame_allocator<R>(f : impl FnOnce(&mut StackFrameAllocator) -> R) -> R {
    let _irq = FrameAllocatorInterruptGuard::new();
    let cell = get_frame_allocator_cell();
    #[cfg(feature = "kernel-arch")]
    let cpu = arch::cpu::current_cpu_id().raw();
    #[cfg(not(feature = "kernel-arch"))]
    let cpu = 0;
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
    debug::lock_acquired(cpu,
                         debug::DebugLockKind::FrameAllocator,
                         object);
    let result = f(&mut allocator);
    drop(allocator);
    debug::lock_released(cpu,
                         debug::DebugLockKind::FrameAllocator,
                         object);
    result
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
    ref_counts : Vec<usize>,
    start_ppn : usize,
    end_ppn : usize,
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
               next_novel : 0 }
    }

    /// 将可用帧限制为半开区间 `[start_ppn,
    /// end_ppn)`（PPN）；会清空回收栈并重置惰性上界。
    pub fn init(&mut self, start_ppn : PhysPageNum, end_ppn : PhysPageNum) {
        self.recycled
            .clear();
        self.start_ppn = start_ppn.0;
        self.end_ppn = end_ppn.0;
        self.next_novel = end_ppn.0;
        self.allocated
            .clear();
        self.allocated
            .resize(end_ppn.0
                           .saturating_sub(start_ppn.0),
                    false);
        self.ref_counts
            .clear();
        self.ref_counts
            .resize(end_ppn.0
                           .saturating_sub(start_ppn.0),
                    0);
    }

    #[inline]
    fn index(&self, frame : PhysPageNum) -> Option<usize> {
        if frame.0 < self.start_ppn || frame.0 >= self.end_ppn {
            None
        } else {
            Some(frame.0 - self.start_ppn)
        }
    }

    /// 只读内存池统计：总帧 = 区间大小；空闲 = 回收栈 + 未分配连续段。
    pub fn mem_stats(&self) -> FrameMemStats {
        let total_frames = self.end_ppn
                               .saturating_sub(self.start_ppn);
        let novel_free = self.next_novel
                             .saturating_sub(self.start_ppn);
        let free_frames = self.recycled
                              .len()
                              .saturating_add(novel_free);
        FrameMemStats { total_frames,
                        free_frames : free_frames.min(total_frames),
                        page_bytes : PAGE_SIZE }
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

    fn alloc_contiguous(&mut self,
                        frame_count : usize,
                        alignment_frames : usize)
                        -> FrameAllocResult<FrameSpan<Self::FrameId>> {
        if frame_count == 0 || alignment_frames == 0 || !alignment_frames.is_power_of_two() {
            return Err(FrameAllocError::InvalidFrame);
        }

        if let Some(max_start) = self.next_novel
                                     .checked_sub(frame_count)
        {
            let start = max_start & !(alignment_frames - 1);
            if start >= self.start_ppn {
                let old_next = self.next_novel;
                let end = start.checked_add(frame_count)
                               .ok_or(FrameAllocError::InvalidFrame)?;
                for ppn in end..old_next {
                    self.recycled
                        .push(PhysPageNum(ppn));
                }
                self.next_novel = start;
                for ppn in start..end {
                    let index = ppn - self.start_ppn;
                    self.allocated[index] = true;
                    self.ref_counts[index] = 1;
                }
                return Ok(FrameSpan::new(PhysPageNum(start), frame_count));
            }
        }

        let last_start = self.end_ppn
                             .checked_sub(frame_count)
                             .ok_or(FrameAllocError::OutOfMemory)?;
        for start in self.next_novel..=last_start {
            if start % alignment_frames != 0 {
                continue;
            }
            let end = start + frame_count;
            if (start..end).all(|ppn| {
                               let index = ppn - self.start_ppn;
                               !self.allocated[index] && self.ref_counts[index] == 0
                           })
            {
                self.recycled
                    .retain(|frame| frame.0 < start || frame.0 >= end);
                for ppn in start..end {
                    let index = ppn - self.start_ppn;
                    self.allocated[index] = true;
                    self.ref_counts[index] = 1;
                }
                return Ok(FrameSpan::new(PhysPageNum(start), frame_count));
            }
        }
        Err(FrameAllocError::OutOfMemory)
    }

    fn dealloc_contiguous(&mut self, span : FrameSpan<Self::FrameId>) -> FrameAllocResult<()> {
        let start = span.start().0;
        let end = start.checked_add(span.frame_count())
                       .ok_or(FrameAllocError::InvalidFrame)?;
        if span.frame_count() == 0 || start < self.start_ppn || end > self.end_ppn {
            return Err(FrameAllocError::InvalidFrame);
        }
        for ppn in start..end {
            let index = ppn - self.start_ppn;
            if !self.allocated[index] || self.ref_counts[index] != 1 {
                return Err(FrameAllocError::InvalidFrame);
            }
        }
        for ppn in start..end {
            let index = ppn - self.start_ppn;
            self.allocated[index] = false;
            self.ref_counts[index] = 0;
            self.recycled
                .push(PhysPageNum(ppn));
        }
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
        Ok(self.ref_counts[idx])
    }

    pub fn ref_count(&self, frame : PhysPageNum) -> FrameAllocResult<usize> {
        let Some(idx) = self.index(frame) else {
            return Err(FrameAllocError::InvalidFrame);
        };
        if !self.allocated[idx] || self.ref_counts[idx] == 0 {
            return Err(FrameAllocError::InvalidFrame);
        }
        Ok(self.ref_counts[idx])
    }
}

// ===== 全局单例（BSP 初始化，运行期多核加锁）=====

static mut FRAME_ALLOCATOR : MaybeUninit<MultiprocessorSafeCell<StackFrameAllocator>> =
    MaybeUninit::uninit();
static FRAME_ALLOCATOR_READY : AtomicBool = AtomicBool::new(false);

// BOOT_CONTRACT: `FRAME_ALLOCATOR` 只能由 BSP 在开放 AP 前初始化。`READY` 的
// Release/Acquire 负责发布已构造对象，不把并发的首次初始化变成安全操作。
fn get_frame_allocator_cell() -> &'static MultiprocessorSafeCell<StackFrameAllocator> {
    assert!(FRAME_ALLOCATOR_READY.load(Ordering::Acquire),
            "frame allocator not initialized: call init_frame_allocator() first");
    unsafe { &*FRAME_ALLOCATOR.as_ptr() }
}

/// 初始化全局帧分配器（临时 stack 实现）。
pub fn init_frame_allocator(start_ppn : PhysPageNum, end_ppn : PhysPageNum) {
    if !FRAME_ALLOCATOR_READY.load(Ordering::Acquire) {
        unsafe {
            FRAME_ALLOCATOR.write(MultiprocessorSafeCell::new(StackFrameAllocator::new()));
        }
        FRAME_ALLOCATOR_READY.store(true, Ordering::Release);
    }
    get_frame_allocator_cell();
    with_frame_allocator(|allocator| allocator.init(start_ppn, end_ppn));
}

/// 获取全局帧分配器单例容器（供特殊场景直接拿 `exclusive_access()`）。
pub fn frame_allocator_cell() -> &'static MultiprocessorSafeCell<StackFrameAllocator> {
    get_frame_allocator_cell()
}

/// 分配一个物理帧（返回帧标识）。
pub fn frame_alloc() -> Option<PhysPageNum> {
    with_frame_allocator(|allocator| {
        allocator.alloc_frame()
                 .ok()
    })
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
    with_frame_allocator(|allocator| allocator.alloc_frame())
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

pub fn frame_alloc_contiguous(frame_count : usize,
                              alignment_frames : usize)
                              -> FrameAllocResult<FrameSpan<PhysPageNum>> {
    with_frame_allocator(|allocator| allocator.alloc_contiguous(frame_count, alignment_frames))
}

pub fn frame_dealloc_contiguous(span : FrameSpan<PhysPageNum>) -> FrameAllocResult<()> {
    with_frame_allocator(|allocator| allocator.dealloc_contiguous(span))
}

#[cfg(test)]
mod contiguous_tests {
    use super::*;

    fn allocator() -> StackFrameAllocator {
        let mut allocator = StackFrameAllocator::new();
        allocator.init(PhysPageNum(3), PhysPageNum(35));
        allocator
    }

    #[test]
    fn allocates_aligned_span_atomically_and_preserves_gap_frames() {
        let mut allocator = allocator();
        let span = allocator.alloc_contiguous(4, 8)
                            .unwrap();
        assert_eq!(span, FrameSpan::new(PhysPageNum(24), 4));
        assert_eq!(allocator.mem_stats()
                            .free_frames,
                   28);
        assert_eq!(allocator.alloc_frame(),
                   Ok(PhysPageNum(34)));
        assert_eq!(allocator.alloc_frame(),
                   Ok(PhysPageNum(33)));
    }

    #[test]
    fn reuses_released_contiguous_run_and_rejects_partial_or_shared_release() {
        let mut allocator = StackFrameAllocator::new();
        allocator.init(PhysPageNum(27), PhysPageNum(35));
        let span = allocator.alloc_contiguous(4, 4)
                            .unwrap();
        allocator.dealloc_contiguous(span)
                 .unwrap();
        assert_eq!(allocator.dealloc_contiguous(span),
                   Err(FrameAllocError::InvalidFrame));
        let reused = allocator.alloc_contiguous(4, 4)
                              .unwrap();
        assert_eq!(reused, span);
        allocator.inc_ref(PhysPageNum(reused.start().0 + 1))
                 .unwrap();
        assert_eq!(allocator.dealloc_contiguous(reused),
                   Err(FrameAllocError::InvalidFrame));
        assert_eq!(allocator.ref_count(reused.start()),
                   Ok(1));
        assert_eq!(allocator.ref_count(PhysPageNum(reused.start().0 + 1)),
                   Ok(2));
    }

    #[test]
    fn invalid_or_impossible_request_does_not_mutate_allocator() {
        let mut allocator = allocator();
        let before = allocator.mem_stats();
        assert_eq!(allocator.alloc_contiguous(0, 1),
                   Err(FrameAllocError::InvalidFrame));
        assert_eq!(allocator.alloc_contiguous(2, 3),
                   Err(FrameAllocError::InvalidFrame));
        assert_eq!(allocator.alloc_contiguous(64, 1),
                   Err(FrameAllocError::OutOfMemory));
        assert_eq!(allocator.mem_stats(), before);
    }
}

/// 全局帧池只读统计；未初始化时返回零值。
pub fn frame_mem_stats() -> FrameMemStats {
    if !FRAME_ALLOCATOR_READY.load(Ordering::Acquire) {
        return FrameMemStats { page_bytes : PAGE_SIZE,
                               ..FrameMemStats::default() };
    }
    with_frame_allocator(|allocator| allocator.mem_stats())
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
