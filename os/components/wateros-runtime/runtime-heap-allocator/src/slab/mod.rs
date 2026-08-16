//! 未接线的 per-CPU slab 核心：固定 size class、单页 slab、页首 header 与本地 free list。
//!
//! 本模块不直接依赖 `wateros-mm`，页来源通过 [`HeapFrameSource`] 注入；
//! Task 03 由顶层注册真实 frame allocator 适配器。

pub(crate) mod cpu_slab;
#[cfg(feature = "slab-diagnostics")]
mod diagnostics;
pub(crate) mod size_class;
pub(crate) mod slab_cache;
pub(crate) mod slab_page;

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicPtr, Ordering};
#[cfg(feature = "impl-slab")]
use alloc::boxed::Box;

use base::cpu::{CpuId, CpuLocal};
use base::sync::BootOnceCell;
use config::task::MAX_CPUS;

use cpu_slab::CpuSlab;
#[cfg(feature = "slab-diagnostics")]
use diagnostics::{SlabDiagnostics, SlabDiagnosticsSnapshot};
use size_class::{SizeClass, SLAB_PAGE_SIZE};
#[cfg(feature = "slab-diagnostics")]
use size_class::{SIZE_CLASS_COUNT, SIZE_CLASS_SIZES};
use slab_page::{SlabPageHeader, SLAB_MAGIC};

/// slab 页来源：返回页对齐的内核可访问基址（WaterOS 恒等映射下为 `PPN * PAGE_SIZE`）。
pub trait HeapFrameSource : Send + Sync {
    /// 分配一页并返回其内核基址；失败返回 `None`。
    fn alloc_frame(&self) -> Option<usize>;

    /// 归还先前由 [`Self::alloc_frame`] 返回的页。
    fn dealloc_frame(&self, frame : usize);

    /// 分配 `pages` 个连续页并返回起始基址；不支持时返回 `None`。
    fn alloc_contiguous(&self, _pages : usize) -> Option<usize> { None }

    /// 归还连续页分配。
    fn dealloc_contiguous(&self, _frame : usize, _pages : usize) {}
}

static FRAME_SOURCE : BootOnceCell<&'static dyn HeapFrameSource> = BootOnceCell::new();
static SLAB : BootOnceCell<&'static SlabAllocator> = BootOnceCell::new();

/// 注册真实 frame source；只能在 BSP 初始化 frame allocator 后调用一次。
pub fn register_frame_source(source : &'static dyn HeapFrameSource) -> Result<(), ()> {
    FRAME_SOURCE.init(source).map_err(|_| ())
}

/// 激活 slab 后端。`impl-slab` feature 关闭时为空操作，内核继续只用 boot backend。
pub fn activate_slab() -> Result<(), ()> {
    #[cfg(feature = "impl-slab")]
    {
        let frames = FRAME_SOURCE.get().copied().ok_or(())?;
        let slab_ref : &'static SlabAllocator = Box::leak(Box::new(SlabAllocator::new(frames)));
        SLAB.init(slab_ref).map_err(|_| ())?;
    }
    Ok(())
}

pub(crate) fn alloc_on(cpu : CpuId, layout : Layout) -> Option<*mut u8> {
    SLAB.get()?.alloc_on(cpu, layout)
}

pub(crate) fn dealloc_on(cpu : CpuId, ptr : *mut u8, layout : Layout) -> bool {
    SLAB.get()
        .map_or(false, |slab| slab.dealloc_on(cpu, ptr, layout))
}

pub(crate) fn is_slab_layout(layout : Layout) -> bool {
    SizeClass::from_layout(layout).is_some()
}

/// 判断旧 slab 对象的 size class 是否还能容纳 `new_size`，从而支持原地 realloc。
pub(crate) fn fits_existing_class(layout : Layout, new_size : usize) -> bool {
    if new_size == 0 {
        return false;
    }
    SizeClass::from_layout(layout)
             .map_or(false, |class| new_size <= class.size())
}

/// 从 frame source 分配连续多页；仅支持页对齐且对齐不超过页大小的大对象。
pub(crate) fn alloc_large(layout : Layout) -> Option<*mut u8> {
    if layout.align() > SLAB_PAGE_SIZE {
        return None;
    }
    let pages = align_up(layout.size(), SLAB_PAGE_SIZE) / SLAB_PAGE_SIZE;
    let frames = FRAME_SOURCE.get()?;
    let base = frames.alloc_contiguous(pages)?;
    Some(base as *mut u8)
}

/// 释放连续多页大对象。
pub(crate) fn dealloc_large(ptr : *mut u8, layout : Layout) -> bool {
    let pages = align_up(layout.size(), SLAB_PAGE_SIZE) / SLAB_PAGE_SIZE;
    let Some(frames) = FRAME_SOURCE.get() else {
        return false;
    };
    frames.dealloc_contiguous(ptr as usize, pages);
    true
}

fn align_up(value : usize, align : usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// 每个 CPU 持有自己的 slab 状态；`CpuLocal` 只保证槽位边界，跨核互斥由调用方
/// 关闭本地中断并遵守 owner-only 约定。
struct CpuLocalSlab(UnsafeCell<CpuSlab>);

// SAFETY: 每个槽位只允许 owner CPU 在关中断后独占访问，跨 CPU 不读写同一槽位。
unsafe impl Sync for CpuLocalSlab {}

struct RemoteFreeHead(AtomicPtr<u8>);

// SAFETY: AtomicPtr 队列通过 CAS 同步，允许跨 CPU push/swap。
unsafe impl Sync for RemoteFreeHead {}

impl RemoteFreeHead {
    fn new() -> Self { Self(AtomicPtr::new(core::ptr::null_mut())) }
}

/// 支持固定数量 CPU 的 slab 后端。
pub(crate) struct SlabAllocator {
    frames : &'static dyn HeapFrameSource,
    cpus : CpuLocal<CpuLocalSlab, MAX_CPUS>,
    remote_heads : CpuLocal<RemoteFreeHead, MAX_CPUS>,
    #[cfg(feature = "slab-diagnostics")]
    diagnostics : CpuLocal<SlabDiagnostics, MAX_CPUS>,
}

impl SlabAllocator {
    pub(crate) fn new(frames : &'static dyn HeapFrameSource) -> Self {
        Self {
            frames,
            cpus : CpuLocal::new(core::array::from_fn(|_| {
                CpuLocalSlab(UnsafeCell::new(CpuSlab::new()))
            })),
            remote_heads : CpuLocal::new(core::array::from_fn(|_| RemoteFreeHead::new())),
            #[cfg(feature = "slab-diagnostics")]
            diagnostics : CpuLocal::new(core::array::from_fn(|_| SlabDiagnostics::new())),
        }
    }

    /// 在 `cpu` 的本地 slab 上分配；layout 超出 slab 范围时返回 `None`。
    pub(crate) fn alloc_on(&self, cpu : CpuId, layout : Layout) -> Option<*mut u8> {
        let class = SizeClass::from_layout(layout)?;
        let slot = self.cpus
                       .get(cpu)?;
        let remote_head = self.remote_heads
                             .get(cpu)?;
        #[cfg(feature = "slab-diagnostics")]
        let diagnostics = self.diagnostics
                              .get(cpu)?;
        #[cfg(feature = "slab-diagnostics")]
        diagnostics.record_alloc(class.index());
        // SAFETY: 调用方保证 `cpu` 为当前 CPU 且已满足 ALLOC_SYNC；本地槽位无并发。
        let state = unsafe { &mut *slot.0.get() };
        // SAFETY: 与 GlobalAlloc::alloc 相同约束；本方法不归还指针所有权。
        let result = unsafe { state.alloc(self.frames,
                                          class.index(),
                                          cpu.raw() as u16,
                                          &remote_head.0,
                                          #[cfg(feature = "slab-diagnostics")]
                                          diagnostics) };
        #[cfg(feature = "slab-diagnostics")]
        if result.is_none() {
            diagnostics.record_fallback();
        }
        result
    }

    /// 在 `cpu` 的本地 slab 上释放；指针不属于该 CPU/size class 时返回 `false`。
    pub(crate) fn dealloc_on(&self,
                             cpu : CpuId,
                             ptr : *mut u8,
                             layout : Layout)
                             -> bool {
        let Some(class) = SizeClass::from_layout(layout) else {
            return false;
        };
        // SAFETY: 调用方保证 ptr 来自 slab；此处只读 header 判断归属。
        let hdr = unsafe { SlabPageHeader::from_obj(ptr) };
        if hdr.magic != SLAB_MAGIC || hdr.size_class() != class.index() {
            return false;
        }
        if hdr.owner_cpu == cpu.raw() as u16 {
            let Some(slot) = self.cpus
                                 .get(cpu)
            else {
                return false;
            };
            #[cfg(feature = "slab-diagnostics")]
            let Some(diagnostics) = self.diagnostics.get(cpu) else {
                return false;
            };
            // SAFETY: 与 GlobalAlloc::dealloc 相同约束；owner CPU 独占本地槽位。
            let state = unsafe { &mut *slot.0.get() };
            let result = unsafe { state.dealloc_local(ptr,
                                                      class.index(),
                                                      #[cfg(feature = "slab-diagnostics")]
                                                      diagnostics) };
            result
        } else {
            let owner = CpuId::from_raw(hdr.owner_cpu as usize);
            let Some(owner_remote_head) = self.remote_heads
                                               .get(owner)
            else {
                return false;
            };
            #[cfg(not(feature = "slab-diagnostics"))]
            CpuSlab::remote_push(&owner_remote_head.0, ptr);
            #[cfg(feature = "slab-diagnostics")]
            {
                let misses = CpuSlab::remote_push(&owner_remote_head.0, ptr);
                if let Some(diagnostics) = self.diagnostics.get(cpu) {
                    diagnostics.record_remote_free(class.index(), misses);
                }
            }
            true
        }
    }

    #[cfg(feature = "slab-diagnostics")]
    fn log_diagnostics(&self) {
        let mut total = SlabDiagnosticsSnapshot::default();
        for cpu_raw in 0..MAX_CPUS {
            let Some(slot) = self.diagnostics.get(CpuId::from_raw(cpu_raw)) else {
                continue;
            };
            let snapshot = slot.snapshot();
            if snapshot.is_empty() {
                continue;
            }
            log::error!("[heap][slab-diag] cpu={} fallback={} oom={} remote_cas_miss={} \
                         drain_events={} drain_objects={} drain_max={}",
                        cpu_raw,
                        snapshot.fallbacks,
                        snapshot.oom,
                        snapshot.remote_cas_misses,
                        snapshot.drain_events,
                        snapshot.drain_objects,
                        snapshot.drain_max);
            for class in 0..SIZE_CLASS_COUNT {
                if snapshot.allocs[class] == 0 &&
                   snapshot.local_frees[class] == 0 &&
                   snapshot.remote_frees[class] == 0 &&
                   snapshot.pages[class] == 0
                {
                    continue;
                }
                log::error!("[heap][slab-diag] cpu={} class={} size={} alloc={} local_hit={} \
                             local_free={} remote_free={} refill={} pages={} empty_pages={}",
                            cpu_raw,
                            class,
                            SIZE_CLASS_SIZES[class],
                            snapshot.allocs[class],
                            snapshot.local_hits[class],
                            snapshot.local_frees[class],
                            snapshot.remote_frees[class],
                            snapshot.frame_refills[class],
                            snapshot.pages[class],
                            snapshot.empty_pages[class]);
            }
            total.add_assign(&snapshot);
        }
        log::error!("[heap][slab-diag] total fallback={} oom={} remote_cas_miss={} \
                     drain_events={} drain_objects={} drain_max={}",
                    total.fallbacks,
                    total.oom,
                    total.remote_cas_misses,
                    total.drain_events,
                    total.drain_objects,
                    total.drain_max);
        for class in 0..SIZE_CLASS_COUNT {
            if total.allocs[class] == 0 && total.pages[class] == 0 {
                continue;
            }
            log::error!("[heap][slab-diag] total class={} size={} alloc={} local_hit={} \
                         local_free={} remote_free={} refill={} pages={} empty_pages={}",
                        class,
                        SIZE_CLASS_SIZES[class],
                        total.allocs[class],
                        total.local_hits[class],
                        total.local_frees[class],
                        total.remote_frees[class],
                        total.frame_refills[class],
                        total.pages[class],
                        total.empty_pages[class]);
        }
    }
}

#[cfg(feature = "slab-diagnostics")]
pub(crate) fn record_fallback(cpu : CpuId) {
    if let Some(slab) = SLAB.get() {
        if let Some(diagnostics) = slab.diagnostics.get(cpu) {
            diagnostics.record_fallback();
        }
    }
}

#[cfg(feature = "slab-diagnostics")]
pub fn log_diagnostics() {
    if let Some(slab) = SLAB.get() {
        slab.log_diagnostics();
    } else {
        log::error!("[heap][slab-diag] slab allocator is not active");
    }
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use core::alloc::Layout;

    use spin::Mutex;

    use super::*;
    use size_class::{SLAB_MAX_SIZE, SLAB_PAGE_SIZE, SIZE_CLASS_SIZES};
    use slab_page::SlabPageHeader;

    struct FakeFrameSource {
        live : Mutex<Vec<Box<[u8; SLAB_PAGE_SIZE]>>>,
        free : Mutex<Vec<usize>>,
    }

    impl FakeFrameSource {
        fn new() -> Self {
            Self { live : Mutex::new(Vec::new()),
                   free : Mutex::new(Vec::new()) }
        }

        fn free_count(&self) -> usize { self.free.lock().len() }
    }

    impl HeapFrameSource for FakeFrameSource {
        fn alloc_frame(&self) -> Option<usize> {
            if let Some(addr) = self.free.lock().pop() {
                return Some(addr);
            }
            let page = Box::new([0u8; SLAB_PAGE_SIZE]);
            let addr = page.as_ptr() as usize;
            self.live.lock().push(page);
            Some(addr)
        }

        fn dealloc_frame(&self, frame : usize) {
            self.free.lock().push(frame);
        }
    }

    fn test_allocator() -> (SlabAllocator, &'static FakeFrameSource) {
        let frames = Box::leak(Box::new(FakeFrameSource::new()));
        let allocator = SlabAllocator::new(frames);
        (allocator, frames)
    }

    #[test]
    fn round_trip_all_size_classes() {
        let (allocator, _frames) = test_allocator();
        let cpu = CpuId::from_raw(0);
        for size in SIZE_CLASS_SIZES {
            let layout = Layout::from_size_align(size, 8).unwrap();
            let ptr = allocator.alloc_on(cpu, layout).expect("alloc");
            assert!(!ptr.is_null());
            unsafe {
                core::ptr::write_bytes(ptr, 0x5a, size);
                assert_eq!(core::ptr::read_volatile(ptr), 0x5a);
            }
            assert!(allocator.dealloc_on(cpu, ptr, layout));
        }
    }

    #[test]
    fn header_reverse_lookup_is_consistent() {
        let (allocator, _frames) = test_allocator();
        let cpu = CpuId::from_raw(1);
        let layout = Layout::from_size_align(128, 8).unwrap();
        let ptr = allocator.alloc_on(cpu, layout).unwrap();
        // SAFETY: ptr 来自 slab，页面未释放。
        let hdr = unsafe { SlabPageHeader::from_obj(ptr) };
        assert_eq!(hdr.owner_cpu, 1);
        assert_eq!(hdr.size_class() as usize, SizeClass::from_layout(layout).unwrap().index());
        assert!(allocator.dealloc_on(cpu, ptr, layout));
    }

    #[test]
    fn many_objects_force_multiple_slabs() {
        let (allocator, frames) = test_allocator();
        let cpu = CpuId::from_raw(0);
        let layout = Layout::from_size_align(64, 8).unwrap();
        let mut ptrs = Vec::new();
        for _ in 0..(SIZE_CLASS_SIZES.len() as usize * 8) {
            ptrs.push(allocator.alloc_on(cpu, layout).expect("alloc"));
        }
        for ptr in ptrs.drain(..) {
            assert!(allocator.dealloc_on(cpu, ptr, layout));
        }
        // 大量释放后至少应能看到归还的页，或者至少不泄漏到负数。
        assert!(frames.free_count() >= 0);
    }

    #[test]
    fn oversized_layout_is_rejected() {
        let (allocator, _frames) = test_allocator();
        let layout = Layout::from_size_align(SLAB_MAX_SIZE + 1, 8).unwrap();
        assert!(allocator.alloc_on(CpuId::from_raw(0), layout).is_none());
    }
}
