//! 固定静态池的 per-CPU TLSF 后端。
//!
//! `KERNEL_HEAP_SIZE` 表示每个 arena 的容量：一个 early/global arena，加上
//! `MAX_CPUS` 个 CPU arena。新分配在显式切换前走 global，切换后优先走当前 CPU；
//! dealloc/realloc 始终按指针地址回到 owner arena。

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, addr_of, addr_of_mut, NonNull};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use config::mm::KERNEL_HEAP_SIZE;
use config::task::MAX_CPUS;
use rlsf::Tlsf;
use spin::Mutex;

use crate::interrupt_guard::{maybe_warn_high_water, with_allocator_interrupt_guard};
use crate::{HeapMemStats, HEAP_SPACE, PER_CPU_HEAP_SPACE};

type KernelTlsf = Tlsf<'static, u32, u32, 23, 32>;

struct TlsfArena {
    inner : Mutex<KernelTlsf>,
    pool_len : AtomicUsize,
    used_estimate : AtomicUsize,
}

impl TlsfArena {
    const fn new() -> Self {
        Self { inner : Mutex::new(KernelTlsf::new()),
               pool_len : AtomicUsize::new(0),
               used_estimate : AtomicUsize::new(0) }
    }

    unsafe fn init(&self, start : *mut u8) {
        let block = NonNull::new(start).expect("per-CPU heap start");
        let block_slice = NonNull::new(ptr::slice_from_raw_parts_mut(block.as_ptr(),
                                                                     KERNEL_HEAP_SIZE))
                              .expect("per-CPU heap slice");
        let pool_len = unsafe {
            self.inner
                .lock()
                .insert_free_block_ptr(block_slice)
                .expect("per-CPU heap pool too small for TLSF")
                .get()
        };
        self.pool_len
            .store(pool_len, Ordering::Release);
        self.used_estimate
            .store(0, Ordering::Release);
    }

    fn stats(&self) -> HeapMemStats {
        let used = self.used_estimate
                       .load(Ordering::Relaxed);
        let free = self.pool_len
                       .load(Ordering::Acquire)
                       .saturating_sub(used);
        HeapMemStats { used,
                       free,
                       capacity : KERNEL_HEAP_SIZE }
    }

    fn allocate(&self, layout : Layout) -> *mut u8 {
        let ptr = self.inner
                      .lock()
                      .allocate(layout)
                      .map_or(ptr::null_mut(), |ptr| ptr.as_ptr());
        if !ptr.is_null() {
            estimate_add(&self.used_estimate, layout.size());
        }
        ptr
    }

    unsafe fn deallocate(&self, ptr : *mut u8, layout : Layout) {
        unsafe {
            self.inner
                .lock()
                .deallocate(NonNull::new_unchecked(ptr),
                            layout.align());
        }
        estimate_sub(&self.used_estimate, layout.size());
    }

    unsafe fn reallocate(&self, ptr : *mut u8, layout : Layout, new_layout : Layout) -> *mut u8 {
        let result = unsafe {
            self.inner
                .lock()
                .reallocate(NonNull::new_unchecked(ptr), new_layout)
                .map_or(ptr::null_mut(), |ptr| ptr.as_ptr())
        };
        if !result.is_null() {
            estimate_sub(&self.used_estimate, layout.size());
            estimate_add(&self.used_estimate, new_layout.size());
        }
        result
    }
}

fn estimate_add(estimate : &AtomicUsize, n : usize) {
    let _ = estimate.fetch_update(Ordering::Relaxed,
                                  Ordering::Relaxed,
                                  |used| Some(used.saturating_add(n)));
}

fn estimate_sub(estimate : &AtomicUsize, n : usize) {
    let _ = estimate.fetch_update(Ordering::Relaxed,
                                  Ordering::Relaxed,
                                  |used| Some(used.saturating_sub(n)));
}

#[derive(Clone, Copy)]
enum ArenaOwner {
    Global,
    Cpu(usize),
}

pub(crate) struct PerCpuTlsfHeap {
    global : TlsfArena,
    cpus : [TlsfArena; MAX_CPUS],
    per_cpu_enabled : AtomicBool,
}

impl PerCpuTlsfHeap {
    const fn new() -> Self {
        Self { global : TlsfArena::new(),
               cpus : [const { TlsfArena::new() }; MAX_CPUS],
               per_cpu_enabled : AtomicBool::new(false) }
    }

    fn arena(&self, owner : ArenaOwner) -> &TlsfArena {
        match owner {
            ArenaOwner::Global => &self.global,
            ArenaOwner::Cpu(cpu) => &self.cpus[cpu],
        }
    }

    fn allocation_owner(&self) -> ArenaOwner {
        if !self.per_cpu_enabled
                .load(Ordering::Acquire)
        {
            return ArenaOwner::Global;
        }
        let cpu = arch::cpu::current_cpu_id();
        assert!(cpu.fits_capacity(MAX_CPUS),
                "per-CPU heap: invalid CPU id {}",
                cpu.raw());
        ArenaOwner::Cpu(cpu.index())
    }

    fn allocate_with_fallback(&self, layout : Layout) -> (*mut u8, HeapMemStats) {
        let owner = self.allocation_owner();
        let arena = self.arena(owner);
        let mut stats = arena.stats();
        let mut ptr = arena.allocate(layout);
        if ptr.is_null() && !matches!(owner, ArenaOwner::Global) {
            stats = self.global.stats();
            ptr = self.global
                      .allocate(layout);
        }
        (ptr, stats)
    }

    fn owner_for_pointer(&self, ptr : *mut u8, layout : Layout) -> Option<ArenaOwner> {
        let value = ptr as usize;
        let global_start = addr_of!(HEAP_SPACE) as usize;
        if range_contains(global_start, value, layout) {
            return Some(ArenaOwner::Global);
        }

        let per_cpu_start = addr_of!(PER_CPU_HEAP_SPACE) as usize;
        let offset = value.checked_sub(per_cpu_start)?;
        let cpu = offset / KERNEL_HEAP_SIZE;
        if cpu < MAX_CPUS &&
           range_contains(per_cpu_start + cpu * KERNEL_HEAP_SIZE,
                          value,
                          layout)
        {
            Some(ArenaOwner::Cpu(cpu))
        } else {
            None
        }
    }

    unsafe fn init(&self) {
        with_allocator_interrupt_guard(|| unsafe {
            self.global
                .init(addr_of_mut!(HEAP_SPACE).cast::<u8>());
            let base = addr_of_mut!(PER_CPU_HEAP_SPACE).cast::<u8>();
            for cpu in 0..MAX_CPUS {
                self.cpus[cpu].init(base.add(cpu * KERNEL_HEAP_SIZE));
            }
            self.per_cpu_enabled
                .store(false, Ordering::Release);
        });
    }

    fn stats(&self) -> HeapMemStats {
        let mut stats = self.global.stats();
        for arena in &self.cpus {
            let cpu = arena.stats();
            stats.used = stats.used
                              .saturating_add(cpu.used);
            stats.free = stats.free
                              .saturating_add(cpu.free);
            stats.capacity = stats.capacity
                                  .saturating_add(cpu.capacity);
        }
        stats
    }
}

fn range_contains(start : usize, ptr : usize, layout : Layout) -> bool {
    let Some(end) = start.checked_add(KERNEL_HEAP_SIZE) else {
        return false;
    };
    ptr >= start &&
    ptr < end &&
    ptr & (layout.align() - 1) == 0 &&
    ptr.checked_add(layout.size())
       .is_some_and(|ptr_end| ptr_end <= end)
}

#[cfg(not(feature = "tlsf-diagnostics"))]
static INVALID_POINTER_WARNED : AtomicBool = AtomicBool::new(false);

fn reject_invalid_pointer(op : &str, ptr : *mut u8, layout : Layout) {
    #[cfg(feature = "tlsf-diagnostics")]
    panic!("[heap] invalid per-CPU TLSF {op} ptr={ptr:p} size={} align={}",
           layout.size(),
           layout.align());
    #[cfg(not(feature = "tlsf-diagnostics"))]
    if !INVALID_POINTER_WARNED.swap(true, Ordering::Relaxed) {
        log::warn!("[heap] ignored invalid per-CPU TLSF {op} ptr={ptr:p} size={} align={}",
                   layout.size(),
                   layout.align());
    }
}

unsafe impl GlobalAlloc for PerCpuTlsfHeap {
    unsafe fn alloc(&self, layout : Layout) -> *mut u8 {
        let (ptr, stats) = with_allocator_interrupt_guard(|| self.allocate_with_fallback(layout));
        maybe_warn_high_water(stats.used, stats.free);
        ptr
    }

    unsafe fn dealloc(&self, ptr : *mut u8, layout : Layout) {
        if ptr.is_null() {
            return;
        }
        let Some(owner) = self.owner_for_pointer(ptr, layout) else {
            reject_invalid_pointer("dealloc", ptr, layout);
            return;
        };
        with_allocator_interrupt_guard(|| unsafe {
            self.arena(owner)
                .deallocate(ptr, layout);
        });
    }

    unsafe fn realloc(&self, ptr : *mut u8, layout : Layout, new_size : usize) -> *mut u8 {
        if ptr.is_null() {
            let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
                return ptr::null_mut();
            };
            return unsafe { self.alloc(new_layout) };
        }
        let Some(owner) = self.owner_for_pointer(ptr, layout) else {
            reject_invalid_pointer("realloc", ptr, layout);
            return ptr::null_mut();
        };
        let (result, stats) = with_allocator_interrupt_guard(|| unsafe {
            let arena = self.arena(owner);
            let stats = arena.stats();
            if new_size == 0 {
                arena.deallocate(ptr, layout);
                return (ptr::null_mut(), stats);
            }
            let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
                return (ptr::null_mut(), stats);
            };
            let result = arena.reallocate(ptr, layout, new_layout);
            if !result.is_null() {
                return (result, stats);
            }

            let (replacement, replacement_stats) = self.allocate_with_fallback(new_layout);
            if replacement.is_null() {
                return (ptr::null_mut(), replacement_stats);
            }
            ptr::copy_nonoverlapping(ptr,
                                     replacement,
                                     layout.size()
                                           .min(new_size));
            arena.deallocate(ptr, layout);
            (replacement, replacement_stats)
        });
        maybe_warn_high_water(stats.used, stats.free);
        result
    }
}

#[global_allocator]
pub(crate) static HEAP_ALLOCATOR : PerCpuTlsfHeap = PerCpuTlsfHeap::new();

pub(crate) fn init_heap() {
    unsafe {
        HEAP_ALLOCATOR.init();
    }
}

pub(crate) fn enable_per_cpu_arenas() {
    HEAP_ALLOCATOR.per_cpu_enabled
                  .store(true, Ordering::Release);
}

pub(crate) fn reserved_end() -> usize {
    addr_of!(PER_CPU_HEAP_SPACE) as usize + MAX_CPUS * KERNEL_HEAP_SIZE
}

pub(crate) fn stats() -> HeapMemStats { HEAP_ALLOCATOR.stats() }
