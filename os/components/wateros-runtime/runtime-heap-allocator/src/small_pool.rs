//! Per-CPU fast path for allocations that fit in one 16-byte slot.
//!
//! The pool owns the first [`POOL_SIZE`] bytes of `HEAP_SPACE`; TLSF must never
//! receive a pointer from this range.  Callers serialize local state by entering
//! the allocator interrupt guard before invoking `allocate` or `deallocate`.

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::ptr::{addr_of_mut, read, write};
use core::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

use base::cpu::{CpuId, CpuLocal};
use config::task::MAX_CPUS;

use crate::HEAP_SPACE;

pub(crate) const SLOT_SIZE : usize = 16;
pub(crate) const POOL_SIZE : usize = 16 * 1024 * 1024;
const SLOT_COUNT : usize = POOL_SIZE / SLOT_SIZE;
const BATCH_SLOTS : usize = 64;

const _ : () = assert!(POOL_SIZE % SLOT_SIZE == 0);

struct LocalPool {
    free_head : usize,
    bump_next : usize,
    bump_end : usize,
}

impl LocalPool {
    const fn new() -> Self {
        Self { free_head : 0,
               bump_next : 0,
               bump_end : 0 }
    }
}

static LOCAL_POOLS : CpuLocal<LocalPool, MAX_CPUS> =
    CpuLocal::from_cells([const { UnsafeCell::new(LocalPool::new()) }; MAX_CPUS]);
// Kept separate from `LOCAL_POOLS` so a diagnostic snapshot never creates a
// shared reference that aliases the current CPU's mutable freelist state.
static LIVE_SLOTS : CpuLocal<AtomicIsize, MAX_CPUS> =
    CpuLocal::from_cells([const { UnsafeCell::new(AtomicIsize::new(0)) }; MAX_CPUS]);
static NEXT_SLOT : AtomicUsize = AtomicUsize::new(0);

#[inline]
fn pool_start() -> usize { addr_of_mut!(HEAP_SPACE) as usize }

#[inline]
pub(crate) fn tlsf_start() -> *mut u8 {
    unsafe { (addr_of_mut!(HEAP_SPACE) as *mut u8).add(POOL_SIZE) }
}

#[inline]
pub(crate) const fn tlsf_len() -> usize { config::mm::KERNEL_HEAP_SIZE - POOL_SIZE }

#[inline]
pub(crate) fn eligible(layout : Layout) -> bool {
    layout.size() != 0 && layout.size() <= SLOT_SIZE && layout.align() <= SLOT_SIZE
}

#[inline]
pub(crate) fn contains(ptr : *mut u8) -> bool {
    let start = pool_start();
    let value = ptr as usize;
    value >= start && value < start + POOL_SIZE
}

#[inline]
pub(crate) fn valid_pointer(ptr : *mut u8, layout : Layout) -> bool {
    contains(ptr) && eligible(layout) && (ptr as usize - pool_start()) % SLOT_SIZE == 0
}

fn local_state() -> (&'static mut LocalPool, &'static AtomicIsize) {
    let cpu = arch::cpu::current_cpu_id();
    let pool = unsafe {
        LOCAL_POOLS.get_local_mut(cpu)
                   .unwrap_or_else(|| panic!("small heap pool: invalid CPU id {}", cpu.raw()))
    };
    let live = LIVE_SLOTS.get(cpu)
                         .unwrap_or_else(|| {
                             panic!("small heap pool live count: invalid CPU id {}", cpu.raw())
                         });
    (pool, live)
}

fn adjust_live(live : &AtomicIsize, delta : isize) {
    // Only the current CPU writes this slot while interrupts are disabled;
    // atomic load/store merely make diagnostic snapshots data-race-free.
    let current = live.load(Ordering::Relaxed);
    live.store(current.saturating_add(delta), Ordering::Relaxed);
}

/// Resets boot-time state before the remaining heap is inserted into TLSF.
///
/// # Safety
/// Must run exactly once on the BSP before APs or heap users can execute.
pub(crate) unsafe fn init() {
    NEXT_SLOT.store(0, Ordering::Release);
    for raw in 0..MAX_CPUS {
        let local = unsafe {
            LOCAL_POOLS.get_local_mut(CpuId::from_raw(raw))
                       .expect("small heap pool CPU slot")
        };
        local.free_head = 0;
        local.bump_next = 0;
        local.bump_end = 0;
        LIVE_SLOTS.get(CpuId::from_raw(raw))
                  .expect("small heap pool live slot")
                  .store(0, Ordering::Release);
    }
}

/// Allocates one fixed slot, or returns null when the layout is ineligible or
/// the unassigned portion of the fixed pool is exhausted.
///
/// The caller must hold the allocator interrupt guard.
pub(crate) unsafe fn allocate(layout : Layout) -> *mut u8 {
    if !eligible(layout) {
        return core::ptr::null_mut();
    }

    let (local, live) = local_state();
    if local.free_head != 0 {
        let ptr = local.free_head as *mut u8;
        local.free_head = unsafe { read(ptr.cast::<usize>()) };
        adjust_live(live, 1);
        return ptr;
    }

    if local.bump_next == local.bump_end {
        let Ok(first) = NEXT_SLOT.fetch_update(Ordering::Relaxed,
                                               Ordering::Relaxed,
                                               |next| {
                                                   (next < SLOT_COUNT).then_some((next + BATCH_SLOTS)
                                                       .min(SLOT_COUNT))
                                               })
        else {
            return core::ptr::null_mut();
        };
        local.bump_next = first;
        local.bump_end = (first + BATCH_SLOTS).min(SLOT_COUNT);
    }

    let slot = local.bump_next;
    local.bump_next += 1;
    adjust_live(live, 1);
    (pool_start() + slot * SLOT_SIZE) as *mut u8
}

/// Returns a validated fixed-pool slot to the current CPU's freelist.
///
/// The caller must hold the allocator interrupt guard and prove
/// `valid_pointer(ptr, original_layout)`.
pub(crate) unsafe fn deallocate(ptr : *mut u8) {
    let (local, live) = local_state();
    unsafe { write(ptr.cast::<usize>(), local.free_head) };
    local.free_head = ptr as usize;
    adjust_live(live, -1);
}

pub(crate) fn allocated_bytes() -> usize {
    let live = (0..MAX_CPUS).fold(0isize, |total, raw| {
        let live = LIVE_SLOTS.get(CpuId::from_raw(raw))
                             .expect("small heap pool live slot");
        total.saturating_add(live.load(Ordering::Relaxed))
    });
    live.clamp(0, SLOT_COUNT as isize) as usize * SLOT_SIZE
}

/// Upper bound used only by the allocation high-water warning fast path.
#[inline]
pub(crate) fn reserved_bytes() -> usize {
    NEXT_SLOT.load(Ordering::Relaxed)
             .min(SLOT_COUNT) * SLOT_SIZE
}

#[cfg(test)]
mod tests {
    use core::alloc::Layout;

    use super::{eligible, BATCH_SLOTS, SLOT_COUNT, SLOT_SIZE};

    #[test]
    fn only_small_suitably_aligned_layouts_are_eligible() {
        assert!(eligible(Layout::from_size_align(1, 1).unwrap()));
        assert!(eligible(Layout::from_size_align(SLOT_SIZE, SLOT_SIZE).unwrap()));
        assert!(!eligible(Layout::from_size_align(SLOT_SIZE + 1, 1).unwrap()));
        assert!(!eligible(Layout::from_size_align(1, SLOT_SIZE * 2).unwrap()));
    }

    #[test]
    fn final_batch_is_bounded_by_pool_end() {
        let first = SLOT_COUNT - (BATCH_SLOTS / 2);
        assert_eq!((first + BATCH_SLOTS).min(SLOT_COUNT), SLOT_COUNT);
    }
}
