#![no_std]
//! 内核全局堆：在启用 `impl-linked-list-allocator` 时注册 [`linked_list_allocator::LockedHeap`]，并链接 `kernel_heap` 符号作为后备空间。
//!
//! 堆大小与对齐来自 `wateros-base-config` 的 MM 配置；[`init`] 必须在任何分配前调用一次。
//!
//! **注意**：与之前的 buddy_system_allocator 不同，linked_list_allocator 使用非侵入式空闲链表，
//! 不会被堆内存本身的 use-after-free 破坏空闲链表元数据。

use config::mm::KERNEL_HEAP_SIZE;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::addr_of_mut;
use linked_list_allocator::LockedHeap;


struct InterruptSafeLockedHeap {
    inner : LockedHeap,
}

impl InterruptSafeLockedHeap {
    const fn empty() -> Self { Self { inner: LockedHeap::empty() } }

    unsafe fn init(&self, heap_start : *mut u8, heap_size : usize) {
        with_allocator_interrupt_guard(|| unsafe {
            self.inner
                .lock()
                .init(heap_start, heap_size);
        });
    }
}

unsafe impl GlobalAlloc for InterruptSafeLockedHeap {
    unsafe fn alloc(&self, layout : Layout) -> *mut u8 {
        with_allocator_interrupt_guard(|| unsafe { GlobalAlloc::alloc(&self.inner, layout) })
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

fn with_allocator_interrupt_guard<R>(f : impl FnOnce() -> R) -> R {
    let state = arch::interrupt::read_global_interrupt_state().ok();
    let _ = arch::interrupt::disable_global_interrupt();
    let ret = f();
    if let Some(state) = state {
        let _ = arch::interrupt::restore_global_interrupt_state(state);
    }
    ret
}

#[allow(unused)]
#[global_allocator]
#[cfg(feature = "impl-linked-list-allocator")]
static HEAP_ALLOCATOR : InterruptSafeLockedHeap = InterruptSafeLockedHeap::empty();

/// 堆分配失败路径：由内核 `#[alloc_error_handler]` 委托（见 `wateros` 根 crate），打印布局后 panic。
///
/// **契约**：不在此处尝试恢复或扩容；仅用于将 OOM 转为可诊断的 panic。
pub fn handle_alloc_error(layout : core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}",
           layout);
}

// 链接脚本或 LDFLAGS 可将 `kernel_heap` 映射到 BSS/专用段；此处提供默认可链接符号。
#[allow(unused)]
#[link_name = "kernel_heap"]
static mut HEAP_SPACE : [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

/// 使用静态 `HEAP_SPACE` 初始化堆分配器区域。
///
/// **契约**：仅在单核引导路径、且堆尚未使用时调用；`unsafe` 块要求调用方保证无并发重入。
pub fn init() {
    unsafe {
        // `LockedHeap::init` 要求传入区域物理/虚拟基址与长度；引导阶段地址空间已固定。
        HEAP_ALLOCATOR.init(addr_of_mut!(HEAP_SPACE) as *mut u8,
                            KERNEL_HEAP_SIZE);
    }
}
