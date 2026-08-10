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
#[cfg(feature = "slab-allocator")]
use crate::slab;
#[cfg(feature = "tlsf-diagnostics")]
use crate::tlsf_diagnostics;

/// TLSF 位图参数：`FLLEN=22` 使最大块 ≥ 128 MiB（64-bit GRANULARITY=32）。
type KernelTlsf = Tlsf<'static, u32, u32, 22, 32>;

pub(crate) struct InterruptSafeTlsfHeap {
    inner : Mutex<KernelTlsf>,
    pool_len : AtomicUsize,
    used_estimate : AtomicUsize,
}

/// Rejects pointers outside the backing pool, ranges crossing its end, and
/// pointers that violate the supplied layout alignment.  This deliberately
/// cannot prove that `ptr` is an allocation start and is not a double-free
/// detector; those require allocator metadata instrumentation.
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
        let next = self.used_estimate.load(Ordering::Relaxed).saturating_add(n);
        self.used_estimate.store(next, Ordering::Relaxed);
    }

    /// 饱和减法，避免不成对 free / size 不一致时 underflow wrapping。
    fn estimate_sub(&self, n : usize) {
        let next = self.used_estimate.load(Ordering::Relaxed).saturating_sub(n);
        self.used_estimate.store(next, Ordering::Relaxed);
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
            #[cfg(feature = "slab-allocator")]
            if slab::class_for(layout).is_some() {
                let mut ptr = unsafe {
                    slab::alloc(layout, |span_layout| {
                        let mut tlsf = self.inner.lock();
                        match tlsf.allocate(span_layout) {
                            Some(span) => {
                                self.estimate_add(span_layout.size());
                                span.as_ptr()
                            }
                            None => ptr::null_mut(),
                        }
                    })
                };
                if ptr.is_null() {
                    unsafe {
                        slab::drain_local_all(|span, span_layout| {
                            let mut tlsf = self.inner.lock();
                            tlsf.deallocate(NonNull::new_unchecked(span), span_layout.align());
                            self.estimate_sub(span_layout.size());
                        });
                    }
                    crate::request_remote_slab_drain();
                    ptr = unsafe {
                        slab::alloc(layout, |span_layout| {
                            let mut tlsf = self.inner.lock();
                            match tlsf.allocate(span_layout) {
                                Some(span) => {
                                    self.estimate_add(span_layout.size());
                                    span.as_ptr()
                                }
                                None => ptr::null_mut(),
                            }
                        })
                    };
                }
                if !ptr.is_null() {
                    #[cfg(feature = "tlsf-diagnostics")]
                    tlsf_diagnostics::alloc(layout, true, false);
                    return (ptr, None);
                }
            }
            #[cfg(feature = "tlsf-diagnostics")]
            let (mut tlsf, spun) = match self.inner.try_lock() {
                Some(lock) => (lock, false),
                None => (self.inner.lock(), true),
            };
            #[cfg(not(feature = "tlsf-diagnostics"))]
            let mut tlsf = self.inner.lock();
            #[cfg(feature = "tlsf-diagnostics")]
            tlsf_diagnostics::lock(spun);
            let ptr = match tlsf.allocate(layout) {
                Some(ptr) => {
                    self.estimate_add(layout.size());
                    ptr.as_ptr()
                }
                None => ptr::null_mut(),
            };
            #[cfg(feature = "tlsf-diagnostics")]
            tlsf_diagnostics::alloc(layout, !ptr.is_null(), true);
            (ptr, Some(self.mem_stats()))
        });
        if let Some(stats) = stats {
            maybe_warn_high_water(stats.used, stats.free);
        }
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
        let slab_handled = with_allocator_interrupt_guard(|| unsafe {
            #[cfg(feature = "slab-allocator")]
            if slab::dealloc(ptr, layout, |span, span_layout| {
                let mut tlsf = self.inner.lock();
                tlsf.deallocate(NonNull::new_unchecked(span), span_layout.align());
                self.estimate_sub(span_layout.size());
            }) {
                #[cfg(feature = "tlsf-diagnostics")]
                tlsf_diagnostics::free(layout);
                return true;
            }
            let nn = NonNull::new_unchecked(ptr);
            #[cfg(feature = "tlsf-diagnostics")]
            let (mut tlsf, spun) = match self.inner.try_lock() {
                Some(lock) => (lock, false),
                None => (self.inner.lock(), true),
            };
            #[cfg(not(feature = "tlsf-diagnostics"))]
            let mut tlsf = self.inner.lock();
            #[cfg(feature = "tlsf-diagnostics")]
            tlsf_diagnostics::lock(spun);
            tlsf.deallocate(nn, layout.align());
            self.estimate_sub(layout.size());
            #[cfg(feature = "tlsf-diagnostics")]
            tlsf_diagnostics::free(layout);
            false
        });
        if slab_handled {
            return;
        }
    }

    unsafe fn realloc(&self, ptr : *mut u8, layout : Layout, new_size : usize) -> *mut u8 {
        if !ptr.is_null() && !dealloc_pointer_in_heap(ptr, layout) {
            reject_invalid_pointer("realloc", ptr, layout);
            return ptr::null_mut();
        }
        #[cfg(feature = "slab-allocator")]
        if let Some(old_class) = unsafe { slab::allocation_class(ptr) } {
            if new_size == 0 {
                unsafe { self.dealloc(ptr, layout) };
                #[cfg(feature = "tlsf-diagnostics")]
                with_allocator_interrupt_guard(|| tlsf_diagnostics::slab_realloc(layout, true));
                return ptr::null_mut();
            }
            let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
                #[cfg(feature = "tlsf-diagnostics")]
                with_allocator_interrupt_guard(|| tlsf_diagnostics::slab_realloc(layout, false));
                return ptr::null_mut();
            };
            if slab::class_for(new_layout) == Some(old_class) {
                #[cfg(feature = "tlsf-diagnostics")]
                with_allocator_interrupt_guard(|| tlsf_diagnostics::slab_realloc(layout, true));
                return ptr;
            }
            let new_ptr = unsafe { self.alloc(new_layout) };
            if new_ptr.is_null() {
                #[cfg(feature = "tlsf-diagnostics")]
                with_allocator_interrupt_guard(|| tlsf_diagnostics::slab_realloc(layout, false));
                return ptr::null_mut();
            }
            unsafe { ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size)) };
            unsafe { self.dealloc(ptr, layout) };
            #[cfg(feature = "tlsf-diagnostics")]
            with_allocator_interrupt_guard(|| tlsf_diagnostics::slab_realloc(layout, true));
            return new_ptr;
        }
        let result = with_allocator_interrupt_guard(|| unsafe {
            #[cfg(feature = "tlsf-diagnostics")]
            let (mut tlsf, spun) = match self.inner.try_lock() {
                Some(lock) => (lock, false),
                None => (self.inner.lock(), true),
            };
            #[cfg(not(feature = "tlsf-diagnostics"))]
            let mut tlsf = self.inner.lock();
            #[cfg(feature = "tlsf-diagnostics")]
            tlsf_diagnostics::lock(spun);
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
        });
        #[cfg(feature = "tlsf-diagnostics")]
        with_allocator_interrupt_guard(|| {
            tlsf_diagnostics::realloc(layout, new_size, !result.is_null());
        });
        result
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

#[cfg(feature = "slab-allocator")]
pub(crate) unsafe fn drain_local_slabs() {
    unsafe {
        slab::drain_local_all(|span, span_layout| {
            let mut tlsf = HEAP_ALLOCATOR.inner.lock();
            tlsf.deallocate(NonNull::new_unchecked(span), span_layout.align());
            HEAP_ALLOCATOR.estimate_sub(span_layout.size());
        });
    }
}
