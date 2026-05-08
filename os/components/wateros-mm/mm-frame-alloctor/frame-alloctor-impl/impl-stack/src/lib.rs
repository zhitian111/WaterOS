//! 全局栈式物理帧分配器（单核 `UniprocessorSafeCell`）：`FrameId` 为 **PPN**，与 4 KiB 物理页一一对应。
//!
//! 调用方须保证 `init_frame_allocator` 传入的 PPN 区间落在 **内核可安全分配/回收** 的 RAM 内；本模块不校验与页表或设备的重叠。

#![no_std]
#![allow(static_mut_refs)]
extern crate alloc;

use alloc::vec::Vec;

use api_v0::{FrameAllocError, FrameAllocResult, PhysicalFrameAllocator};
use mm_api::addr::PhysPageNum;
use wateros_base::addr::BasePPN;
use wateros_base::sync::UniprocessorSafeCell;

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

/// 临时的 LIFO 栈式帧分配器：
/// - `init(start_ppn, end_ppn)` 产生 free-list；
/// - `alloc_frame()` 从栈顶 pop；
/// - `dealloc_frame()` push 回栈（顺序回放）。
///
/// 注意：该实现未做“重复释放/未分配校验”，属于早期阶段可接受的简化。
///
/// 空闲帧来自两段语义：尚未动过的连续高段 `[start_ppn, next_novel)`（惰性下推），
/// 以及显式回收栈 `recycled`。不在 `init` 时把整段 PPN 推入 `Vec`，避免大内存下撑爆内核 heap。
pub struct StackFrameAllocator {
    recycled: Vec<PhysPageNum>,
    start_ppn: usize,
    /// 仍可从连续区分配的第一页号上界（不包含）；初始为 `end_ppn`。
    next_novel: usize,
}

impl StackFrameAllocator {
    /// 构造空分配器；须再调用 [`Self::init`] 方可从 PPN 区间取帧。
    pub fn new() -> Self {
        Self {
            recycled: Vec::new(),
            start_ppn: 0,
            next_novel: 0,
        }
    }

    /// 将可用帧限制为半开区间 `[start_ppn, end_ppn)`（PPN）；会清空回收栈并重置惰性上界。
    pub fn init(&mut self, start_ppn: BasePPN, end_ppn: BasePPN) {
        self.recycled.clear();
        self.start_ppn = start_ppn.val;
        self.next_novel = end_ppn.val;
    }
}

impl PhysicalFrameAllocator for StackFrameAllocator {
    type FrameId = PhysPageNum;

    fn alloc_frame(&mut self) -> FrameAllocResult<Self::FrameId> {
        if let Some(p) = self.recycled.pop() {
            return Ok(p);
        }
        if self.next_novel > self.start_ppn {
            self.next_novel -= 1;
            return Ok(PhysPageNum(self.next_novel));
        }
        Err(FrameAllocError::OutOfMemory)
    }

    fn dealloc_frame(&mut self, frame: Self::FrameId) -> FrameAllocResult<()> {
        // early stage：不校验重复释放
        self.recycled.push(frame);
        Ok(())
    }
}

// ===== 全局单例（安全单核）=====

static mut FRAME_ALLOCATOR: MaybeUninit<UniprocessorSafeCell<StackFrameAllocator>> =
    MaybeUninit::uninit();
static FRAME_ALLOCATOR_READY: AtomicBool = AtomicBool::new(false);

// `FRAME_ALLOCATOR` 仅在首次 `init_frame_allocator` 中 `write` 一次，之后只读；与 `READY` 发布顺序配对。
fn get_frame_allocator_cell() -> &'static UniprocessorSafeCell<StackFrameAllocator> {
    assert!(
        FRAME_ALLOCATOR_READY.load(Ordering::Acquire),
        "frame allocator not initialized: call init_frame_allocator() first"
    );
    unsafe { &*FRAME_ALLOCATOR.as_ptr() }
}

/// 初始化全局帧分配器（临时 stack 实现）。
pub fn init_frame_allocator(start_ppn: BasePPN, end_ppn: BasePPN) {
    if !FRAME_ALLOCATOR_READY.load(Ordering::Acquire) {
        unsafe {
            FRAME_ALLOCATOR.write(UniprocessorSafeCell::new(StackFrameAllocator::new()));
        }
        FRAME_ALLOCATOR_READY.store(true, Ordering::Release);
    }
    get_frame_allocator_cell()
        .exclusive_access()
        .init(start_ppn, end_ppn);
}

/// 获取全局帧分配器单例容器（供特殊场景直接拿 `exclusive_access()`）。
pub fn frame_allocator_cell() -> &'static UniprocessorSafeCell<StackFrameAllocator> {
    get_frame_allocator_cell()
}

/// 分配一个物理帧（返回帧标识）。
pub fn frame_alloc() -> Option<PhysPageNum> {
    get_frame_allocator_cell()
        .exclusive_access()
        .alloc_frame()
        .ok()
}

/// 回收一个物理帧。
pub fn frame_dealloc(frame: PhysPageNum) {
    let _ = frame_allocator_cell()
        .exclusive_access()
        .dealloc_frame(frame);
}

pub fn frame_alloc_result() -> FrameAllocResult<PhysPageNum> {
    get_frame_allocator_cell()
        .exclusive_access()
        .alloc_frame()
}

pub fn frame_dealloc_result(frame: PhysPageNum) -> FrameAllocResult<()> {
    get_frame_allocator_cell()
        .exclusive_access()
        .dealloc_frame(frame)
}

pub fn test_with_range(start_ppn: BasePPN, end_ppn: BasePPN) {
    log::trace!("[frame-alloctor::impl-stack] test begin");
    log::trace!(
        "[frame-alloctor::impl-stack] init range: [{:#x}, {:#x})",
        start_ppn.val,
        end_ppn.val
    );

    init_frame_allocator(start_ppn, end_ppn);

    let cap = end_ppn.val.saturating_sub(start_ppn.val);
    if cap == 0 {
        log::info!("[frame-alloctor::impl-stack] empty range, skip alloc test");
        log::trace!("[frame-alloctor::impl-stack] test end");
        return;
    }

    // 大内存下不要用单个 Vec 持有全部帧（元数据会占满内核 heap）。
    const BATCH: usize = 256;
    let mut buf: Vec<PhysPageNum> = Vec::new();
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
    const MAX_FULL_HOLD: usize = 8192;
    if cap <= MAX_FULL_HOLD {
        let mut frames: Vec<PhysPageNum> = Vec::new();
        for _ in 0..cap {
            frames.push(frame_alloc_result().expect("alloc before OOM"));
        }
        assert!(
            frame_alloc_result().is_err(),
            "should be OOM after exhausting frames"
        );
        for f in frames.drain(..) {
            frame_dealloc_result(f).expect("dealloc should succeed");
        }
        let _ = frame_alloc_result().expect("alloc should succeed after recycle");
    } else {
        log::info!(
            "[frame-alloctor::impl-stack] skip full OOM witness ({} pages > {})",
            cap,
            MAX_FULL_HOLD
        );
    }

    log::trace!("[frame-alloctor::impl-stack] test end");
}
