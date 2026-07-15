#![no_std]
//! 内核全局堆：默认 [`linked_list_allocator::LockedHeap`]（非侵入式空闲链表）；
//! 可通过 feature `impl-tlsf` 切换为 [`rlsf::Tlsf`]（O(1) alloc/dealloc）。
//!
//! 堆大小与对齐来自 `wateros-base-config` 的 MM 配置；[`init`] 必须在任何分配前调用一次。

mod interrupt_guard;
mod stress;

use config::mm::KERNEL_HEAP_SIZE;

#[cfg(all(feature = "impl-tlsf", feature = "impl-linked-list-allocator"))]
compile_error!("enable only one of `impl-tlsf` or `impl-linked-list-allocator`");

#[cfg(not(any(feature = "impl-tlsf", feature = "impl-linked-list-allocator")))]
compile_error!("enable `impl-linked-list-allocator` (default) or `impl-tlsf`");

#[cfg(feature = "impl-linked-list-allocator")]
mod backend_linked_list;
#[cfg(feature = "impl-tlsf")]
mod backend_tlsf;

#[cfg(feature = "impl-linked-list-allocator")]
use backend_linked_list as backend;
#[cfg(feature = "impl-tlsf")]
use backend_tlsf as backend;

pub use stress::heap_fragmentation_stress_report;

/// 内核堆用量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapMemStats {
    /// 已分配字节（实现定义：链表后端为精确值，TLSF 为估算）。
    pub used : usize,
    /// 剩余可用字节。
    pub free : usize,
    /// 堆池总容量（`KERNEL_HEAP_SIZE`）。
    pub capacity : usize,
}

pub(crate) use backend::HEAP_ALLOCATOR;

/// 返回当前内核堆用量（`used`/`free`/`capacity`）。
pub fn heap_mem_stats() -> HeapMemStats {
    interrupt_guard::with_allocator_interrupt_guard(|| backend::stats())
}

/// 堆分配失败路径：由内核 `#[alloc_error_handler]` 委托（见 `wateros` 根 crate），打印布局后 panic。
pub fn handle_alloc_error(layout : core::alloc::Layout) -> ! {
    let stats = heap_mem_stats();
    log::warn!("[heap] OOM: layout_size={} align={} used={} free={} cap={}",
               layout.size(),
               layout.align(),
               stats.used,
               stats.free,
               stats.capacity);
    panic!("Heap allocation error, layout = {:?}",
           layout);
}

// 128 MiB 堆池单独段 `.kernel.heap`，由链接脚本放在 BSS 末尾，避免堆越界覆盖
// SCHEDULER 等小型内核全局变量（见 platform link.ld）。
#[allow(unused)]
#[link_name = "kernel_heap"]
#[unsafe(link_section = ".kernel.heap")]
pub(crate) static mut HEAP_SPACE : [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

unsafe extern "C" {
    static kernel_heap_start : u8;
    static kernel_heap_end : u8;
}

/// 使用静态 `HEAP_SPACE` 初始化堆分配器区域。
///
/// **契约**：仅在单核引导路径、且堆尚未使用时调用；`unsafe` 块要求调用方保证无并发重入。
pub fn init() {
    let heap_lo = unsafe { core::ptr::addr_of!(kernel_heap_start) as usize };
    let heap_hi = unsafe { core::ptr::addr_of!(kernel_heap_end) as usize };
    log::warn!("[boot-init] kernel_heap pool [{:#x},{:#x}) cap={:#x}",
               heap_lo,
               heap_hi,
               heap_hi.saturating_sub(heap_lo));
    backend::init_heap();
}
