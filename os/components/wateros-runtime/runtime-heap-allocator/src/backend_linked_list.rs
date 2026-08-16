//! 基于 `linked_list_allocator::LockedHeap` 的内核全局堆后端。
//!
//! **不变量**：所有 `GlobalAlloc` 路径经 [`crate::interrupt_guard`] 关中断并检测递归分配。

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::addr_of_mut;

use config::mm::KERNEL_HEAP_SIZE;
use linked_list_allocator::LockedHeap;

use crate::heap_backend::HeapBackend;
use crate::interrupt_guard::{maybe_warn_high_water, with_allocator_interrupt_guard};
use crate::HeapMemStats;
use crate::HEAP_SPACE;

pub(crate) struct InterruptSafeLockedHeap {
    inner : LockedHeap,
}

impl InterruptSafeLockedHeap {
    pub(crate) const fn empty() -> Self { Self { inner: LockedHeap::empty() } }

    pub(crate) fn mem_stats_impl(&self) -> HeapMemStats {
        let heap = self.inner.lock();
        HeapMemStats { used: heap.used(),
                       free: heap.free(),
                       capacity: KERNEL_HEAP_SIZE,
                       slab_retained: 0,
                       slab_reclaimable: 0,
                       frame_used: 0,
                       frame_free: 0,
                       frame_capacity: 0 }
    }

    pub(crate) unsafe fn init_region(&self,
                                     heap_start : *mut u8,
                                     heap_size : usize) {
        with_allocator_interrupt_guard(|| unsafe {
            self.inner
                .lock()
                .init(heap_start, heap_size);
        });
    }
}

impl HeapBackend for InterruptSafeLockedHeap {
    fn init(&self) {
        unsafe {
            self.init_region(addr_of_mut!(HEAP_SPACE) as *mut u8,
                             KERNEL_HEAP_SIZE);
        }
    }

    fn mem_stats(&self) -> HeapMemStats {
        self.mem_stats_impl()
    }

    unsafe fn alloc(&self, layout : Layout) -> *mut u8 {
        let (ptr, used, free) = with_allocator_interrupt_guard(|| {
            let heap = self.inner.lock();
            let used = heap.used();
            let free = heap.free();
            drop(heap);
            (unsafe { GlobalAlloc::alloc(&self.inner, layout) }, used, free)
        });
        maybe_warn_high_water(used, free);
        ptr
    }

    unsafe fn dealloc(&self, ptr : *mut u8, layout : Layout) {
        with_allocator_interrupt_guard(|| unsafe { GlobalAlloc::dealloc(&self.inner, ptr, layout) })
    }

    unsafe fn realloc(&self,
                      ptr : *mut u8,
                      layout : Layout,
                      new_size : usize)
                      -> *mut u8 {
        with_allocator_interrupt_guard(|| unsafe {
            GlobalAlloc::realloc(&self.inner, ptr, layout, new_size)
        })
    }
}

pub(crate) static ACTIVE_ALLOCATOR : InterruptSafeLockedHeap = InterruptSafeLockedHeap::empty();

pub(crate) fn init_heap() {
    ACTIVE_ALLOCATOR.init();
}

pub(crate) fn stats() -> HeapMemStats { ACTIVE_ALLOCATOR.mem_stats() }
