//! 基于 `rlsf::Tlsf` 的 O(1) 内核堆后端；`used` 为原子估算值。
//!
//! **不变量**：分配路径与 [`crate::interrupt_guard`] 一致；`pool_len` 在 `init` 后不变。

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, addr_of_mut, NonNull};
#[cfg(not(feature = "tlsf-diagnostics"))]
use core::sync::atomic::AtomicBool;
use core::sync::atomic::{AtomicUsize, Ordering};

use config::mm::KERNEL_HEAP_SIZE;
use rlsf::Tlsf;
use spin::Mutex;

use crate::interrupt_guard::{maybe_warn_high_water, with_allocator_interrupt_guard};
use crate::HeapMemStats;
use crate::HEAP_SPACE;

/// TLSF 位图参数：`FLLEN=23` 覆盖统一配置的 256 MiB 静态堆。
type KernelTlsf = Tlsf<'static, u32, u32, 23, 32>;

pub(crate) struct InterruptSafeTlsfHeap {
    /// TLSF 元数据锁；禁止在锁内触发再次分配。
    inner : Mutex<KernelTlsf>,
    /// 初始化后固定的实际池长度。
    pool_len : AtomicUsize,
    /// 按 layout 大小累计的近似已用字节数。
    used_estimate : AtomicUsize,
}

/// 拒绝落在堆池外、跨越池尾或不满足给定布局对齐要求的指针。
/// 该检查不能证明指针是分配起点，也不是 double-free 检测器；后者需要额外的
/// allocator 元数据诊断。
fn dealloc_pointer_in_heap(ptr : *mut u8, layout : Layout) -> bool {
    let heap_start = addr_of_mut!(HEAP_SPACE) as usize;
    let heap_end = heap_start.checked_add(KERNEL_HEAP_SIZE)
                             .unwrap_or(usize::MAX);
    let ptr_value = ptr as usize;
    ptr_value >= heap_start &&
    ptr_value < heap_end &&
    ptr_value & (layout.align() - 1) == 0 &&
    ptr_value.checked_add(layout.size())
             .map(|end| end <= heap_end)
             .unwrap_or(false)
}

#[cfg(not(feature = "tlsf-diagnostics"))]
static INVALID_POINTER_WARNED : AtomicBool = AtomicBool::new(false);

fn reject_invalid_pointer(op : &str, ptr : *mut u8, layout : Layout) {
    #[cfg(feature = "tlsf-diagnostics")]
    panic!("[heap] invalid TLSF {op} ptr={ptr:p} size={} align={}",
           layout.size(),
           layout.align());
    #[cfg(not(feature = "tlsf-diagnostics"))]
    if !INVALID_POINTER_WARNED.swap(true, Ordering::Relaxed) {
        log::warn!("[heap] ignored invalid TLSF {op} ptr={ptr:p} size={} align={}",
                   layout.size(),
                   layout.align());
    }
}

impl InterruptSafeTlsfHeap {
    pub(crate) const fn new() -> Self {
        Self { inner : Mutex::new(KernelTlsf::new()),
               pool_len : AtomicUsize::new(0),
               used_estimate : AtomicUsize::new(0) }
    }

    pub(crate) fn mem_stats(&self) -> HeapMemStats {
        let used = self.used_estimate
                       .load(Ordering::Relaxed);
        let pool_len = self.pool_len
                           .load(Ordering::Acquire);
        let free = pool_len.saturating_sub(used);
        HeapMemStats { used,
                       free,
                       capacity : KERNEL_HEAP_SIZE }
    }

    /// 饱和加法，避免诊断用估算值 wrapping 成天文数字。
    fn estimate_add(&self, n : usize) {
        let _ = self.used_estimate
                    .fetch_update(Ordering::Relaxed,
                                  Ordering::Relaxed,
                                  |u| Some(u.saturating_add(n)));
    }

    /// 饱和减法，避免不成对 free / size 不一致时 underflow wrapping。
    fn estimate_sub(&self, n : usize) {
        let _ = self.used_estimate
                    .fetch_update(Ordering::Relaxed,
                                  Ordering::Relaxed,
                                  |u| Some(u.saturating_sub(n)));
    }

    pub(crate) unsafe fn init(&self) {
        with_allocator_interrupt_guard(|| unsafe {
            let block = NonNull::new_unchecked(addr_of_mut!(HEAP_SPACE) as *mut u8);
            let block_slice = NonNull::new(ptr::slice_from_raw_parts_mut(block.as_ptr(),
                                                                          KERNEL_HEAP_SIZE))
                                  .expect("heap slice");
            let pool_len = self.inner
                               .lock()
                               .insert_free_block_ptr(block_slice)
                               .expect("heap pool too small for TLSF")
                               .get();
            self.pool_len
                .store(pool_len, Ordering::Release);
            self.used_estimate
                .store(0, Ordering::Release);
        });
    }
}

unsafe impl GlobalAlloc for InterruptSafeTlsfHeap {
    unsafe fn alloc(&self, layout : Layout) -> *mut u8 {
        let (ptr, stats) = with_allocator_interrupt_guard(|| {
            let stats = self.mem_stats();
            let mut tlsf = self.inner.lock();
            let ptr = match tlsf.allocate(layout) {
                Some(ptr) => {
                    self.estimate_add(layout.size());
                    ptr.as_ptr()
                }
                None => ptr::null_mut(),
            };
            (ptr, stats)
        });
        maybe_warn_high_water(stats.used, stats.free);
        ptr
    }

    unsafe fn dealloc(&self, ptr : *mut u8, layout : Layout) {
        if ptr.is_null() {
            return;
        }
        if !dealloc_pointer_in_heap(ptr, layout) {
            reject_invalid_pointer("dealloc", ptr, layout);
            return;
        }
        with_allocator_interrupt_guard(|| unsafe {
            self.estimate_sub(layout.size());
            let nn = NonNull::new_unchecked(ptr);
            self.inner
                .lock()
                .deallocate(nn, layout.align());
        })
    }

    unsafe fn realloc(&self, ptr : *mut u8, layout : Layout, new_size : usize) -> *mut u8 {
        if !ptr.is_null() && !dealloc_pointer_in_heap(ptr, layout) {
            reject_invalid_pointer("realloc", ptr, layout);
            return ptr::null_mut();
        }
        with_allocator_interrupt_guard(|| unsafe {
            let mut tlsf = self.inner.lock();
            if ptr.is_null() {
                let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
                    return ptr::null_mut();
                };
                return match tlsf.allocate(new_layout) {
                    Some(p) => {
                        self.estimate_add(new_layout.size());
                        p.as_ptr()
                    }
                    None => ptr::null_mut(),
                };
            }
            if new_size == 0 {
                self.estimate_sub(layout.size());
                tlsf.deallocate(NonNull::new_unchecked(ptr),
                                layout.align());
                return ptr::null_mut();
            }
            let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
                return ptr::null_mut();
            };
            match tlsf.reallocate(NonNull::new_unchecked(ptr), new_layout) {
                Some(p) => {
                    self.estimate_sub(layout.size());
                    self.estimate_add(new_layout.size());
                    p.as_ptr()
                }
                None => ptr::null_mut(),
            }
        })
    }
}

#[global_allocator]
pub(crate) static HEAP_ALLOCATOR : InterruptSafeTlsfHeap = InterruptSafeTlsfHeap::new();

pub(crate) fn init_heap() {
    unsafe {
        HEAP_ALLOCATOR.init();
    }
}

pub(crate) fn stats() -> HeapMemStats { HEAP_ALLOCATOR.mem_stats() }
