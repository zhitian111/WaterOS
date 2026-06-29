use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use config::mm::KERNEL_HEAP_SIZE;

static HEAP_GUARD_DEPTH : AtomicUsize = AtomicUsize::new(0);
static HEAP_HIGH_WATER_WARNED : AtomicBool = AtomicBool::new(false);

const HEAP_HIGH_WATER_NUMERATOR : usize = 9;
const HEAP_HIGH_WATER_DENOMINATOR : usize = 10;

pub(crate) fn with_allocator_interrupt_guard<R>(f : impl FnOnce() -> R) -> R {
    let depth = HEAP_GUARD_DEPTH.fetch_add(1, Ordering::Acquire);
    if depth > 0 {
        HEAP_GUARD_DEPTH.fetch_sub(1, Ordering::Release);
        panic!("recursive heap allocation detected (depth={})",
               depth + 1);
    }
    let state = arch::interrupt::read_global_interrupt_state().ok();
    let _ = arch::interrupt::disable_global_interrupt();
    let ret = f();
    if let Some(state) = state {
        let _ = arch::interrupt::restore_global_interrupt_state(state);
    }
    HEAP_GUARD_DEPTH.fetch_sub(1, Ordering::Release);
    ret
}

pub(crate) fn maybe_warn_high_water(used : usize, free : usize) {
    if HEAP_HIGH_WATER_WARNED.load(Ordering::Relaxed) {
        return;
    }
    if used > KERNEL_HEAP_SIZE * HEAP_HIGH_WATER_NUMERATOR / HEAP_HIGH_WATER_DENOMINATOR {
        HEAP_HIGH_WATER_WARNED.store(true, Ordering::Relaxed);
        log::warn!("[heap] high water: used={} free={} cap={}",
                   used,
                   free,
                   KERNEL_HEAP_SIZE);
    }
}
