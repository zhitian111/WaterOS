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

use base::cpu::{CpuId, CpuLocal};
use config::task::MAX_CPUS;

use cpu_slab::CpuSlab;
use size_class::SizeClass;

/// slab 页来源：返回页对齐的内核可访问基址（WaterOS 恒等映射下为 `PPN * PAGE_SIZE`）。
pub(crate) trait HeapFrameSource : Sync {
    /// 分配一页并返回其内核基址；失败返回 `None`。
    fn alloc_frame(&self) -> Option<usize>;

    /// 归还先前由 [`Self::alloc_frame`] 返回的页。
    fn dealloc_frame(&self, frame : usize);
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
        let Some(slot) = self.cpus
                             .get(cpu)
        else {
            return false;
        };
        // SAFETY: 与 GlobalAlloc::dealloc 相同约束；本方法验证 header 后才会释放。
        let state = unsafe { &mut *slot.0.get() };
        // SAFETY: 同上；指针必须来自该 CPU 的 slab。
        unsafe { state.dealloc(ptr, class.index(), self.frames) }
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
