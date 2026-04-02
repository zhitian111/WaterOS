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
pub struct StackFrameAllocator {
    recycled: Vec<PhysPageNum>,
}

impl StackFrameAllocator {
    pub fn new() -> Self {
        Self { recycled: Vec::new() }
    }

    pub fn init(&mut self, start_ppn: BasePPN, end_ppn: BasePPN) {
        self.recycled.clear();
        // [start, end) 语义：end 不包含
        for p in start_ppn.val..end_ppn.val {
            self.recycled.push(PhysPageNum(p));
        }
    }
}

impl PhysicalFrameAllocator for StackFrameAllocator {
    type FrameId = PhysPageNum;

    fn alloc_frame(&mut self) -> FrameAllocResult<Self::FrameId> {
        self.recycled.pop().ok_or(FrameAllocError::OutOfMemory)
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

    // 分配到耗尽
    let mut frames: Vec<PhysPageNum> = Vec::new();
    for _ in 0..cap {
        let f = frame_alloc_result().expect("alloc should succeed before OOM");
        frames.push(f);
    }
    assert!(frame_alloc_result().is_err(), "should be OOM after exhausting frames");

    // 释放后应可再次分配
    for f in frames.drain(..) {
        frame_dealloc_result(f).expect("dealloc should succeed");
    }
    let _ = frame_alloc_result().expect("alloc should succeed after recycle");

    log::trace!("[frame-alloctor::impl-stack] test end");
}
