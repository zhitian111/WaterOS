//! One-class per-CPU fixed pool for allocations fitting in 128 bytes.

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::ptr::{addr_of_mut, null_mut};
use core::sync::atomic::{AtomicUsize, Ordering};

use base::cpu::CpuLocal;
use config::task::MAX_CPUS;

pub(crate) const SLOT_SIZE : usize = 128;
const POOL_SIZE : usize = 16 * 1024 * 1024;
const SLOT_COUNT : usize = POOL_SIZE / SLOT_SIZE;
const CHUNK_SLOTS : usize = 32;

#[repr(C, align(128))]
pub(crate) struct SmallPoolStorage([u8; POOL_SIZE]);

#[link_name = "kernel_small_object_pool"]
#[unsafe(link_section = ".kernel.heap")]
static mut STORAGE : SmallPoolStorage = SmallPoolStorage([0; POOL_SIZE]);

struct LocalPool {
    free_head : usize,
    chunk_next : usize,
    chunk_end : usize,
}

impl LocalPool {
    const fn new() -> Self {
        Self { free_head : 0,
               chunk_next : 0,
               chunk_end : 0 }
    }
}

// The allocator interrupt guard makes the current CPU the sole writer of its
// slot. No other CPU keeps a reference to another CPU's LocalPool.
unsafe impl Sync for LocalPool {}

static LOCAL : CpuLocal<LocalPool, MAX_CPUS> =
    CpuLocal::from_cells([const { UnsafeCell::new(LocalPool::new()) }; MAX_CPUS]);
static NEXT_SLOT : AtomicUsize = AtomicUsize::new(0);

#[inline]
pub(crate) fn eligible(layout : Layout) -> bool {
    layout.size() != 0 && layout.size() <= SLOT_SIZE && layout.align() <= SLOT_SIZE
}

#[inline]
fn bounds() -> (usize, usize) {
    let start = addr_of_mut!(STORAGE) as usize;
    (start, start + POOL_SIZE)
}

#[inline]
pub(crate) fn owns(ptr : *mut u8) -> bool {
    let (start, end) = bounds();
    let value = ptr as usize;
    value >= start && value < end && (value - start) % SLOT_SIZE == 0
}

#[inline]
fn local() -> &'static mut LocalPool {
    let cpu = arch::cpu::current_cpu_id();
    unsafe { LOCAL.get_local_mut(cpu).expect("small pool: CPU id exceeds MAX_CPUS") }
}

/// Allocate while the caller holds the allocator interrupt guard.
pub(crate) unsafe fn alloc(layout : Layout) -> *mut u8 {
    if !eligible(layout) {
        return null_mut();
    }
    let local = local();
    if local.free_head != 0 {
        let result = local.free_head;
        local.free_head = unsafe { *(result as *const usize) };
        return result as *mut u8;
    }
    if local.chunk_next != local.chunk_end {
        let result = local.chunk_next;
        local.chunk_next += SLOT_SIZE;
        return result as *mut u8;
    }

    let first = NEXT_SLOT.fetch_add(CHUNK_SLOTS, Ordering::Relaxed);
    if first > SLOT_COUNT.saturating_sub(CHUNK_SLOTS) {
        return null_mut();
    }
    let start = bounds().0 + first * SLOT_SIZE;
    local.chunk_next = start + SLOT_SIZE;
    local.chunk_end = start + CHUNK_SLOTS * SLOT_SIZE;
    start as *mut u8
}

/// Free while the caller holds the allocator interrupt guard.
pub(crate) unsafe fn dealloc(ptr : *mut u8) {
    debug_assert!(owns(ptr));
    let local = local();
    unsafe { *(ptr as *mut usize) = local.free_head };
    local.free_head = ptr as usize;
}

