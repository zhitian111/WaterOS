#![no_std]
//! 内核全局堆：默认使用 [`rlsf::Tlsf`]（O(1) alloc/dealloc）；可通过
//! feature `impl-linked-list-allocator` 切回 [`linked_list_allocator::LockedHeap`]。
//!
//! 堆大小与对齐来自 `wateros-base-config` 的 MM 配置；[`init`] 必须在任何分配前调用一次。
//!
//! RUNTIME_ORDER: `init` 在 BSP 的单线程启动阶段完成后，AP 才可执行可能分配的路径。
//! ALLOC_SYNC: 后端锁保护分配器元数据，`interrupt_guard` 同时禁止本 CPU 的中断重入。

extern crate alloc;

mod heap_backend;
mod interrupt_guard;
mod slab;
mod stress;

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, addr_of_mut};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use config::mm::KERNEL_HEAP_SIZE;
use heap_backend::HeapBackend;

pub use slab::{HeapFrameMemStats, HeapFrameSource};
#[cfg(feature = "slab-diagnostics")]
pub use slab::log_diagnostics as log_slab_diagnostics;

const STATE_BOOT : u8 = 0;
const STATE_SLAB : u8 = 1;

#[cfg(not(test))]
fn current_cpu_id() -> base::cpu::CpuId { arch::cpu::current_cpu_id() }

#[cfg(test)]
fn current_cpu_id() -> base::cpu::CpuId { base::cpu::CpuId::from_raw(0) }

/// 大对象是否走 frame-backed 连续帧路径；默认关闭（boot TLSF 有界，避免
/// 大分配把 guest 内存提交到 QEMU 导致宿主机 OOM）。Task 07 调参时开启。
static LARGE_FRAME_ENABLED : AtomicBool = AtomicBool::new(false);

#[cfg(all(feature = "impl-tlsf", feature = "impl-linked-list-allocator"))]
compile_error!("enable only one of `impl-tlsf` or `impl-linked-list-allocator`");

#[cfg(not(any(feature = "impl-tlsf", feature = "impl-linked-list-allocator")))]
compile_error!("enable `impl-tlsf` (default) or `impl-linked-list-allocator`");

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
    /// boot heap 已分配字节（链表后端为精确值，TLSF 为估算）。
    pub used : usize,
    /// boot heap 剩余可用字节。
    pub free : usize,
    /// boot heap 总容量（`KERNEL_HEAP_SIZE`）。
    pub capacity : usize,
    /// slab 当前持有的物理页字节数，包括部分使用和 warm empty 页。
    pub slab_retained : usize,
    /// slab 脱离 current、作为 warm reserve 保留的全空页字节数。
    pub slab_reclaimable : usize,
    /// 全局 frame pool 视角的已用字节；包含 per-CPU frame batch 缓存页。
    pub frame_used : usize,
    /// 全局 frame pool 的空闲字节；不包含 per-CPU frame batch 缓存页。
    pub frame_free : usize,
    /// 全局 frame pool 总容量字节数。
    pub frame_capacity : usize,
}

/// 全局分配器入口：按编译期 feature 委托给唯一活动后端。
///
/// 该类型保持为无状态门面，后端状态仍由各 backend 模块自己的静态对象持有；
/// Task 03 在这里增加运行期 boot/slab 后端切换。
pub(crate) struct KernelAllocator {
    state : AtomicU8,
}

impl KernelAllocator {
    pub(crate) const fn new() -> Self { Self { state : AtomicU8::new(STATE_BOOT) } }

    fn activate_slab(&self) {
        #[cfg(feature = "impl-slab")]
        self.state.store(STATE_SLAB, Ordering::Release);
    }

    fn slab_active(&self) -> bool {
        #[cfg(feature = "impl-slab")]
        {
            return self.state.load(Ordering::Acquire) == STATE_SLAB;
        }
        #[cfg(not(feature = "impl-slab"))]
        {
            false
        }
    }
}

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout : Layout) -> *mut u8 {
        if self.slab_active() {
            let ptr = interrupt_guard::with_allocator_interrupt_guard(|| {
                if slab::is_slab_layout(layout) {
                    slab::alloc_on(current_cpu_id(), layout)
                } else if LARGE_FRAME_ENABLED.load(Ordering::Acquire) {
                    #[cfg(feature = "slab-diagnostics")]
                    slab::record_fallback(current_cpu_id());
                    slab::alloc_large(layout)
                } else {
                    #[cfg(feature = "slab-diagnostics")]
                    slab::record_fallback(current_cpu_id());
                    None
                }
            });
            if let Some(ptr) = ptr {
                return ptr;
            }
        }
        unsafe { backend::ACTIVE_ALLOCATOR.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr : *mut u8, layout : Layout) {
        if ptr.is_null() {
            return;
        }
        if self.slab_active() && !in_boot_heap(ptr) {
            let freed = interrupt_guard::with_allocator_interrupt_guard(|| {
                if slab::is_slab_layout(layout) {
                    slab::dealloc_on(current_cpu_id(), ptr, layout)
                } else if LARGE_FRAME_ENABLED.load(Ordering::Acquire) {
                    slab::dealloc_large(ptr, layout)
                } else {
                    false
                }
            });
            if freed {
                return;
            }
        }
        unsafe { backend::ACTIVE_ALLOCATOR.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self,
                      ptr : *mut u8,
                      layout : Layout,
                      new_size : usize)
                      -> *mut u8 {
        if !ptr.is_null() && new_size > 0 && self.slab_active() &&
           !in_boot_heap(ptr) && slab::fits_existing_class(layout, new_size)
        {
            return ptr;
        }
        if ptr.is_null() {
            let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
                return ptr::null_mut();
            };
            return unsafe { <Self as GlobalAlloc>::alloc(self, new_layout) };
        }
        if new_size == 0 {
            unsafe { <Self as GlobalAlloc>::dealloc(self, ptr, layout) };
            return ptr::null_mut();
        }
        let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
            return ptr::null_mut();
        };
        let new_ptr = unsafe { <Self as GlobalAlloc>::alloc(self, new_layout) };
        if !new_ptr.is_null() {
            let copy_size = layout.size().min(new_size);
            // SAFETY: 新旧分配均按各自 layout 有效，拷贝不超过旧对象大小。
            unsafe { ptr::copy_nonoverlapping(ptr, new_ptr, copy_size) };
            unsafe { <Self as GlobalAlloc>::dealloc(self, ptr, layout) };
        }
        new_ptr
    }
}

/// 判断指针是否落在静态 boot TLSF 池内。
fn in_boot_heap(ptr : *mut u8) -> bool {
    let start = addr_of_mut!(HEAP_SPACE) as usize;
    let end = start.saturating_add(KERNEL_HEAP_SIZE);
    let value = ptr as usize;
    value >= start && value < end
}

#[cfg_attr(not(test), global_allocator)]
pub(crate) static HEAP_ALLOCATOR : KernelAllocator = KernelAllocator::new();

/// 注册 slab 使用的真实 frame source；由 BSP 在 frame allocator 初始化后调用。
pub fn register_frame_source(source : &'static dyn HeapFrameSource) -> Result<(), ()> {
    slab::register_frame_source(source)
}

/// 激活 slab 后端；`impl-slab` feature 关闭时为空操作。
pub fn activate_slab() -> Result<(), ()> {
    slab::activate_slab()?;
    HEAP_ALLOCATOR.activate_slab();
    #[cfg(feature = "impl-slab")]
    log::info!("[heap] slab backend activated (small objects via frame-backed per-CPU caches)");
    Ok(())
}

/// 在当前 CPU 上有界推进一个 pending remote-free class。
///
/// 由定时器中断调用；普通分配临界区会关闭本 CPU 中断，因此不会与 owner cache 并发。
#[inline]
pub fn maintain_slab_remote_frees() {
    if !HEAP_ALLOCATOR.slab_active() {
        return;
    }
    interrupt_guard::with_allocator_interrupt_guard(|| slab::maintain_on(current_cpu_id()));
}

/// 返回 boot heap、slab retained page 和全局 frame pool 的独立快照。
///
/// 这是诊断快照：拿到值后 allocator 可立即变化；TLSF backend 的 `used` 还是按 layout
/// 大小累计的估算值，不能用于内存回收决策。
pub fn heap_mem_stats() -> HeapMemStats {
    interrupt_guard::with_allocator_interrupt_guard(|| {
        let mut stats = backend::stats();
        let slab_pages = slab::page_mem_stats();
        stats.slab_retained = slab_pages.pages.saturating_mul(slab::size_class::SLAB_PAGE_SIZE);
        stats.slab_reclaimable = slab_pages.reclaimable_pages
                                              .saturating_mul(slab::size_class::SLAB_PAGE_SIZE);
        if let Some(frames) = slab::frame_mem_stats() {
            stats.frame_capacity = frames.capacity;
            stats.frame_free = frames.free.min(frames.capacity);
            stats.frame_used = stats.frame_capacity.saturating_sub(stats.frame_free);
        }
        stats
    })
}

/// 堆分配失败路径：由内核 `#[alloc_error_handler]` 委托（见 `wateros` 根 crate），打印布局后 panic。
pub fn handle_alloc_error(layout : core::alloc::Layout) -> ! {
    let stats = heap_mem_stats();
    log::warn!("[heap] OOM: layout_size={} align={} boot_used={} boot_free={} boot_cap={} \
                slab_retained={} slab_reclaimable={} frame_used={} frame_free={} frame_cap={}",
               layout.size(),
               layout.align(),
               stats.used,
               stats.free,
               stats.capacity,
               stats.slab_retained,
               stats.slab_reclaimable,
               stats.frame_used,
               stats.frame_free,
               stats.frame_capacity);
    panic!("Heap allocation error, layout = {:?}",
           layout);
}

// 128 MiB 堆池单独段 `.kernel.heap`，由链接脚本放在 BSS 末尾，避免堆越界覆盖
// SCHEDULER 等小型内核全局变量（见 platform link.ld）。
#[allow(unused)]
#[link_name = "kernel_heap"]
#[unsafe(link_section = ".kernel.heap")]
pub(crate) static mut HEAP_SPACE : [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

/// 使用静态 `HEAP_SPACE` 初始化堆分配器区域。
///
/// **契约**：仅在单核引导路径、且堆尚未使用时调用；调用方保证无并发重入。
/// 重复初始化会破坏 allocator 元数据，不能作为 AP 初始化步骤调用。
pub fn init() {
    backend::init_heap();
    #[cfg(feature = "stress-on-init")]
    heap_fragmentation_stress_report(100_000);
}

#[cfg(feature = "self_test")]
/// 堆组件可用性自检：申请、写入、校验并释放临时分配。
pub fn self_test() {
    use alloc::boxed::Box;
    log::info!("[heap] self_test begin");
    let mut value = Box::new([0u8; 128]);
    value[0] = 0x5a;
    value[127] = 0xa5;
    assert_eq!(value[0], 0x5a);
    assert_eq!(value[127], 0xa5);
    drop(value);
    log::info!("[heap] self_test complete; allocation released");
}
