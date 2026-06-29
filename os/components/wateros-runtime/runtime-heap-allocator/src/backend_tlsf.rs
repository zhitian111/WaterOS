//! 基于 `rlsf::Tlsf` 的 O(1) 内核堆后端；`used` 为原子估算值。
//!
//! **不变量**：分配路径与 [`crate::interrupt_guard`] 一致；`pool_len` 在 `init` 后不变。

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, addr_of_mut, NonNull};
use core::sync::atomic::{AtomicUsize, Ordering};

use config::mm::KERNEL_HEAP_SIZE;
use rlsf::Tlsf;
use spin::Mutex;

use crate::interrupt_guard::{maybe_warn_high_water, with_allocator_interrupt_guard};
use crate::HeapMemStats;
use crate::HEAP_SPACE;

/// TLSF 位图参数：`FLLEN=22` 使最大块 ≥ 128 MiB（64-bit GRANULARITY=32）。
type KernelTlsf = Tlsf<'static, u32, u32, 22, 32>;

pub(crate) struct InterruptSafeTlsfHeap {
    inner : Mutex<KernelTlsf>,
    pool_len : AtomicUsize,
    used_estimate : AtomicUsize,
}

impl InterruptSafeTlsfHeap {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self { inner: Mutex::new(KernelTlsf::new()),
               pool_len: AtomicUsize::new(0),
               used_estimate: AtomicUsize::new(0) }
    }

    #[inline]
    pub(crate) fn mem_stats(&self) -> HeapMemStats {
        let used = self.used_estimate.load(Ordering::Relaxed);
        let pool_len = self.pool_len.load(Ordering::Acquire);
        let free = pool_len.saturating_sub(used);
        HeapMemStats { used,
                       free,
                       capacity: KERNEL_HEAP_SIZE }
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
        with_allocator_interrupt_guard(|| {
            let stats = self.mem_stats();
            maybe_warn_high_water(stats.used, stats.free);
            let mut tlsf = self.inner.lock();
            match tlsf.allocate(layout) {
                Some(ptr) => {
                    self.used_estimate
                        .fetch_add(layout.size(), Ordering::Relaxed);
                    ptr.as_ptr()
                }
                None => ptr::null_mut(),
            }
        })
    }

    unsafe fn dealloc(&self, ptr : *mut u8, layout : Layout) {
        if ptr.is_null() {
            return;
        }
        with_allocator_interrupt_guard(|| unsafe {
            self.used_estimate
                .fetch_sub(layout.size(), Ordering::Relaxed);
            let nn = NonNull::new_unchecked(ptr);
            self.inner
                .lock()
                .deallocate(nn, layout.align());
        })
    }

    unsafe fn realloc(&self,
                      ptr : *mut u8,
                      layout : Layout,
                      new_size : usize)
                      -> *mut u8 {
        with_allocator_interrupt_guard(|| unsafe {
            let mut tlsf = self.inner.lock();
            if ptr.is_null() {
                let new_layout = Layout::from_size_align(new_size, layout.align())
                                     .unwrap_or(layout);
                return match tlsf.allocate(new_layout) {
                    Some(p) => {
                        self.used_estimate
                            .fetch_add(new_layout.size(), Ordering::Relaxed);
                        p.as_ptr()
                    }
                    None => ptr::null_mut(),
                };
            }
            if new_size == 0 {
                self.used_estimate
                    .fetch_sub(layout.size(), Ordering::Relaxed);
                tlsf.deallocate(NonNull::new_unchecked(ptr), layout.align());
                return ptr::null_mut();
            }
            let new_layout = Layout::from_size_align(new_size, layout.align())
                                 .unwrap_or(layout);
            match tlsf.reallocate(NonNull::new_unchecked(ptr), new_layout) {
                Some(p) => {
                    self.used_estimate
                        .fetch_sub(layout.size(), Ordering::Relaxed);
                    self.used_estimate
                        .fetch_add(new_layout.size(), Ordering::Relaxed);
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

#[inline]
pub(crate) fn stats() -> HeapMemStats { HEAP_ALLOCATOR.mem_stats() }
