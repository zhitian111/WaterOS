//! 基于 `linked_list_allocator::LockedHeap` 的内核全局堆后端。
//!
//! **不变量**：所有 `GlobalAlloc` 路径经 [`crate::interrupt_guard`] 关中断并检测递归分配。

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::addr_of_mut;

use config::mm::KERNEL_HEAP_SIZE;
use linked_list_allocator::LockedHeap;

use crate::interrupt_guard::{maybe_warn_high_water, with_allocator_interrupt_guard};
use crate::HeapMemStats;
use crate::HEAP_SPACE;

pub(crate) struct InterruptSafeLockedHeap {
    /// linked-list allocator 的元数据锁；中断屏蔽由外层 guard 负责。
    inner : LockedHeap,
}

impl InterruptSafeLockedHeap {
    pub(crate) const fn empty() -> Self { Self { inner: LockedHeap::empty() } }

    pub(crate) fn mem_stats(&self) -> HeapMemStats {
        let heap = self.inner.lock();
        HeapMemStats { used: heap.used(),
                       free: heap.free(),
                       capacity: KERNEL_HEAP_SIZE }
    }

    pub(crate) unsafe fn init(&self, heap_start : *mut u8, heap_size : usize) {
        with_allocator_interrupt_guard(|| unsafe {
            self.inner
                .lock()
                .init(heap_start, heap_size);
        });
    }
}

unsafe impl GlobalAlloc for InterruptSafeLockedHeap {
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

#[global_allocator]
pub(crate) static HEAP_ALLOCATOR : InterruptSafeLockedHeap = InterruptSafeLockedHeap::empty();

pub(crate) fn init_heap() {
    unsafe {
        HEAP_ALLOCATOR.init(addr_of_mut!(HEAP_SPACE) as *mut u8,
                            KERNEL_HEAP_SIZE);
    }
}

pub(crate) fn stats() -> HeapMemStats { HEAP_ALLOCATOR.mem_stats() }
