#![no_std]
//! 内核全局堆：在启用 `impl-buddy-allocator` 时注册 [`buddy_system_allocator::LockedHeap`]，并链接 `kernel_heap` 符号作为后备空间。
//!
//! 堆大小与对齐来自 `wateros-base-config` 的 MM 配置；[`init`] 必须在任何分配前调用一次。

use buddy_system_allocator::LockedHeap;
use config::mm::{KERNEL_HEAP_SIZE, KERNEL_HEAP_SIZE_BIT_WIDTH};
use core::ptr::addr_of_mut;


#[allow(unused)]
#[global_allocator]
#[cfg(feature = "impl-buddy-allocator")]
static HEAP_ALLOCATOR : LockedHeap<KERNEL_HEAP_SIZE_BIT_WIDTH> = LockedHeap::new();

/// 堆分配失败路径：由内核 `#[alloc_error_handler]` 委托（见 `wateros` 根 crate），打印布局后 panic。
pub fn handle_alloc_error(layout : core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}",
           layout);
}

#[allow(unused)]
#[link_name = "kernel_heap"]
static mut HEAP_SPACE : [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

/// 使用静态 `HEAP_SPACE` 初始化伙伴分配器区域。
///
/// **契约**：仅在单核引导路径、且堆尚未使用时调用；`unsafe` 块要求调用方保证无并发重入。
pub fn init() {
    unsafe {
        HEAP_ALLOCATOR.lock()
                      .init(addr_of_mut!(HEAP_SPACE) as usize,
                            KERNEL_HEAP_SIZE);
    }
}
