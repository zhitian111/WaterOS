//! 未接线的 per-CPU slab 核心：固定 size class、单页 slab、页首 header 与本地 free list。
//!
//! 本模块不直接依赖 `wateros-mm`，页来源通过 [`HeapFrameSource`] 注入；
//! Task 03 由顶层注册真实 frame allocator 适配器。

pub(crate) mod cpu_slab;
#[cfg(feature = "slab-diagnostics")]
mod diagnostics;
mod page_stats;
pub(crate) mod size_class;
pub(crate) mod slab_cache;
pub(crate) mod slab_page;

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::sync::atomic::AtomicPtr;
#[cfg(feature = "impl-slab")]
use alloc::boxed::Box;

use base::cpu::{CpuId, CpuLocal};
use base::sync::BootOnceCell;
use config::task::MAX_CPUS;

use cpu_slab::CpuSlab;
#[cfg(feature = "slab-diagnostics")]
use diagnostics::{SlabDiagnostics, SlabDiagnosticsSnapshot};
use page_stats::{SlabPageMemStats, SlabPageStats};
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

    /// 返回 frame pool 的只读快照。`free` 不包含仍缓存在 per-CPU frame batch 中的页。
    fn mem_stats(&self) -> Option<HeapFrameMemStats> { None }

    /// 分配 `pages` 个连续页并返回起始基址；不支持时返回 `None`。
    fn alloc_contiguous(&self, _pages : usize) -> Option<usize> { None }

    /// 归还连续页分配。
    fn dealloc_contiguous(&self, _frame : usize, _pages : usize) {}
}

/// slab frame source 的字节级快照；capacity/free 属于全局 frame pool 口径。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeapFrameMemStats {
    pub capacity : usize,
    pub free : usize,
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

pub(crate) fn page_mem_stats() -> SlabPageMemStats {
    SLAB.get()
        .map_or_else(SlabPageMemStats::default, |slab| slab.page_mem_stats())
}

pub(crate) fn frame_mem_stats() -> Option<HeapFrameMemStats> {
    FRAME_SOURCE.get().and_then(|frames| frames.mem_stats())
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
    page_stats : CpuLocal<SlabPageStats, MAX_CPUS>,
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
            page_stats : CpuLocal::new(core::array::from_fn(|_| SlabPageStats::new())),
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
        let page_stats = self.page_stats
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
                                          page_stats,
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
            let Some(page_stats) = self.page_stats.get(cpu) else {
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
                                                      self.frames,
                                                      page_stats,
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

    pub(crate) fn page_mem_stats(&self) -> SlabPageMemStats {
        let mut total = SlabPageMemStats::default();
        for cpu_raw in 0..MAX_CPUS {
            let Some(stats) = self.page_stats.get(CpuId::from_raw(cpu_raw)) else {
                continue;
            };
            let snapshot = stats.snapshot();
            total.pages = total.pages.saturating_add(snapshot.pages);
            total.reclaimable_pages = total.reclaimable_pages
                                           .saturating_add(snapshot.reclaimable_pages);
        }
        total
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
                             local_free={} remote_free={} refill={} reclaim={} pages={} \
                             empty_pages={}",
                            cpu_raw,
                            class,
                            SIZE_CLASS_SIZES[class],
                            snapshot.allocs[class],
                            snapshot.local_hits[class],
                            snapshot.local_frees[class],
                            snapshot.remote_frees[class],
                            snapshot.frame_refills[class],
                            snapshot.frame_reclaims[class],
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
                         local_free={} remote_free={} refill={} reclaim={} pages={} \
                         empty_pages={}",
                        class,
                        SIZE_CLASS_SIZES[class],
                        total.allocs[class],
                        total.local_hits[class],
                        total.local_frees[class],
                        total.remote_frees[class],
                        total.frame_refills[class],
                        total.frame_reclaims[class],
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

    #[repr(align(4096))]
    struct TestPage([u8; SLAB_PAGE_SIZE]);

    struct FakeFrameSource {
        live : Mutex<Vec<Box<TestPage>>>,
        free : Mutex<Vec<usize>>,
    }

    impl FakeFrameSource {
        fn new() -> Self {
            Self { live : Mutex::new(Vec::new()),
                   free : Mutex::new(Vec::new()) }
        }

        fn free_count(&self) -> usize { self.free.lock().len() }

        fn backing_count(&self) -> usize { self.live.lock().len() }
    }

    impl HeapFrameSource for FakeFrameSource {
        fn alloc_frame(&self) -> Option<usize> {
            if let Some(addr) = self.free.lock().pop() {
                return Some(addr);
            }
            let page = Box::new(TestPage([0u8; SLAB_PAGE_SIZE]));
            let addr = page.0.as_ptr() as usize;
            assert_eq!(addr % SLAB_PAGE_SIZE, 0);
            self.live.lock().push(page);
            Some(addr)
        }

        fn dealloc_frame(&self, frame : usize) {
            assert_eq!(frame % SLAB_PAGE_SIZE, 0);
            assert!(self.live.lock().iter().any(|page| page.0.as_ptr() as usize == frame),
                    "returned frame does not belong to source");
            let mut free = self.free.lock();
            assert!(!free.contains(&frame), "frame returned twice");
            free.push(frame);
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

    fn allocate_many(allocator : &SlabAllocator,
                     cpu : CpuId,
                     layout : Layout,
                     count : usize)
                     -> Vec<*mut u8> {
        (0..count).map(|_| allocator.alloc_on(cpu, layout).expect("alloc")).collect()
    }

    fn objects_per_page(layout : Layout) -> usize {
        SizeClass::from_layout(layout).unwrap().objects_per_slab()
    }

    #[test]
    fn fully_empty_pages_are_returned_except_two_warm_pages() {
        let (allocator, frames) = test_allocator();
        let cpu = CpuId::from_raw(0);
        let layout = Layout::from_size_align(64, 8).unwrap();
        let mut ptrs = allocate_many(&allocator, cpu, layout, objects_per_page(layout) * 3);
        assert_eq!(frames.backing_count(), 3);
        for ptr in ptrs.drain(..) {
            assert!(allocator.dealloc_on(cpu, ptr, layout));
        }
        assert_eq!(frames.free_count(), 1);
        let stats = allocator.page_mem_stats();
        assert_eq!(stats.pages, 2);
        assert_eq!(stats.reclaimable_pages, 1);
    }

    #[test]
    fn partially_free_page_is_not_reclaimed() {
        let (allocator, frames) = test_allocator();
        let cpu = CpuId::from_raw(0);
        let layout = Layout::from_size_align(128, 8).unwrap();
        let mut ptrs = allocate_many(&allocator, cpu, layout, objects_per_page(layout) + 1);
        assert!(allocator.dealloc_on(cpu, ptrs.remove(0), layout));
        assert_eq!(frames.free_count(), 0);
        let stats = allocator.page_mem_stats();
        assert_eq!(stats.pages, 2);
        assert_eq!(stats.reclaimable_pages, 0);
        for ptr in ptrs {
            assert!(allocator.dealloc_on(cpu, ptr, layout));
        }
        assert_eq!(frames.free_count(), 0);
    }

    #[test]
    fn remote_frees_reclaim_only_after_owner_drain() {
        let (allocator, frames) = test_allocator();
        let owner = CpuId::from_raw(0);
        let remote = CpuId::from_raw(1);
        let layout = Layout::from_size_align(384, 8).unwrap();
        let ptrs = allocate_many(&allocator, owner, layout, objects_per_page(layout) * 3);
        for ptr in ptrs {
            assert!(allocator.dealloc_on(remote, ptr, layout));
        }
        assert_eq!(frames.free_count(), 0);
        assert_eq!(allocator.page_mem_stats().reclaimable_pages, 0);

        let ptr = allocator.alloc_on(owner, layout).expect("owner drain alloc");
        assert_eq!(frames.free_count(), 1);
        let stats = allocator.page_mem_stats();
        assert_eq!(stats.pages, 2);
        assert_eq!(stats.reclaimable_pages, 1);
        assert!(allocator.dealloc_on(owner, ptr, layout));
    }

    #[test]
    fn reclaimed_frames_are_reused_without_new_backing_pages() {
        let (allocator, frames) = test_allocator();
        let cpu = CpuId::from_raw(0);
        let layout = Layout::from_size_align(768, 8).unwrap();
        let count = objects_per_page(layout) * 3;
        let ptrs = allocate_many(&allocator, cpu, layout, count);
        for ptr in ptrs {
            assert!(allocator.dealloc_on(cpu, ptr, layout));
        }
        assert_eq!(frames.backing_count(), 3);
        assert_eq!(frames.free_count(), 1);

        let ptrs = allocate_many(&allocator, cpu, layout, count);
        assert_eq!(frames.backing_count(), 3);
        assert_eq!(frames.free_count(), 0);
        for ptr in ptrs {
            assert!(allocator.dealloc_on(cpu, ptr, layout));
        }
        assert_eq!(frames.free_count(), 1);
    }

    #[test]
    fn each_size_class_survives_ten_thousand_round_trips() {
        let (allocator, frames) = test_allocator();
        let cpu = CpuId::from_raw(0);
        for size in SIZE_CLASS_SIZES {
            let layout = Layout::from_size_align(size, 8).unwrap();
            for iteration in 0..10_000usize {
                let ptr = allocator.alloc_on(cpu, layout).expect("stress alloc");
                unsafe { ptr.write((iteration & 0xff) as u8) };
                assert!(allocator.dealloc_on(cpu, ptr, layout));
            }
        }
        assert_eq!(frames.backing_count(), SIZE_CLASS_SIZES.len());
        assert_eq!(frames.free_count(), 0);
    }

    #[test]
    fn oversized_layout_is_rejected() {
        let (allocator, _frames) = test_allocator();
        let layout = Layout::from_size_align(SLAB_MAX_SIZE + 1, 8).unwrap();
        assert!(allocator.alloc_on(CpuId::from_raw(0), layout).is_none());
    }
}
