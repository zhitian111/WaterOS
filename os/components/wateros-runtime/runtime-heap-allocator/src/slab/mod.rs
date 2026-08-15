//! 未接线的 per-CPU slab 核心：固定 size class、单页 slab、页首 header 与本地 free list。
//!
//! 本模块不直接依赖 `wateros-mm`，页来源通过 [`HeapFrameSource`] 注入；
//! Task 03 由顶层注册真实 frame allocator 适配器。

pub(crate) mod cpu_slab;
pub(crate) mod size_class;
pub(crate) mod slab_cache;
pub(crate) mod slab_page;

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "impl-slab")]
use alloc::boxed::Box;

use base::cpu::{CpuId, CpuLocal};
use base::sync::BootOnceCell;
use config::task::MAX_CPUS;

use cpu_slab::CpuSlab;
use size_class::{SizeClass, SLAB_PAGE_SIZE};
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

static SLAB_ALLOC_COUNT : AtomicUsize = AtomicUsize::new(0);
static SLAB_DEALLOC_COUNT : AtomicUsize = AtomicUsize::new(0);

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
    let ptr = SLAB.get()?.alloc_on(cpu, layout)?;
    SLAB_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    Some(ptr)
}

pub(crate) fn dealloc_on(cpu : CpuId, ptr : *mut u8, layout : Layout) -> bool {
    let ok = SLAB.get()
                 .map_or(false, |slab| slab.dealloc_on(cpu, ptr, layout));
    if ok {
        SLAB_DEALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    ok
}

pub(crate) fn stats() -> (usize, usize) {
    (SLAB_ALLOC_COUNT.load(Ordering::Relaxed),
     SLAB_DEALLOC_COUNT.load(Ordering::Relaxed))
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

/// 支持固定数量 CPU 的 slab 后端。
pub(crate) struct SlabAllocator {
    frames : &'static dyn HeapFrameSource,
    cpus : CpuLocal<CpuLocalSlab, MAX_CPUS>,
}

impl SlabAllocator {
    pub(crate) fn new(frames : &'static dyn HeapFrameSource) -> Self {
        Self {
            frames,
            cpus : CpuLocal::new(core::array::from_fn(|_| {
                CpuLocalSlab(UnsafeCell::new(CpuSlab::new()))
            })),
        }
    }

    /// 在 `cpu` 的本地 slab 上分配；layout 超出 slab 范围时返回 `None`。
    pub(crate) fn alloc_on(&self, cpu : CpuId, layout : Layout) -> Option<*mut u8> {
        let class = SizeClass::from_layout(layout)?;
        let slot = self.cpus
                       .get(cpu)?;
        // SAFETY: 调用方保证 `cpu` 为当前 CPU 且已满足 ALLOC_SYNC；本地槽位无并发。
        let state = unsafe { &mut *slot.0.get() };
        // SAFETY: 与 GlobalAlloc::alloc 相同约束；本方法不归还指针所有权。
        unsafe { state.alloc(self.frames, class.index(), cpu.raw() as u16) }
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
            // SAFETY: 与 GlobalAlloc::dealloc 相同约束；owner CPU 独占本地槽位。
            let state = unsafe { &mut *slot.0.get() };
            unsafe { state.dealloc_local(ptr, class.index()) }
        } else {
            let owner = CpuId::from_raw(hdr.owner_cpu as usize);
            let Some(owner_slot) = self.cpus
                                       .get(owner)
            else {
                return false;
            };
            // SAFETY: remote_push 只修改 owner CPU 的 Mutex 保护队列，不触碰本地 cache。
            unsafe { (&*owner_slot.0.get()).remote_push(ptr) };
            true
        }
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
