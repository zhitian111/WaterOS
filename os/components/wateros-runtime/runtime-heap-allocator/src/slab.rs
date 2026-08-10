//! Small-object slab front-end for the TLSF heap.

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

use base::cpu::CpuLocal;
use config::task::MAX_CPUS;
use spin::Mutex;

pub(crate) const SPAN_SIZE : usize = 16 * 1024;
const MAGIC : usize = 0x5741_5445_5253_4c42;
const CLASS_SIZES : [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];
const MAG_CAPACITY : [usize; 8] = [128, 128, 64, 64, 32, 16, 8, 4];
const BITMAP_WORDS : usize = 16;

#[repr(C, align(64))]
struct SpanHeader {
    magic : usize,
    class : usize,
    capacity : usize,
    central : AtomicUsize,
    home_cpu : usize,
    next_span : usize,
    _reserved : [usize; 2],
}

const HEADER_SIZE : usize = core::mem::size_of::<SpanHeader>();

#[repr(C, align(64))]
struct AllocationBitmap {
    words : [AtomicUsize; BITMAP_WORDS],
}

#[cfg(feature = "tlsf-diagnostics")]
const BITMAP_SIZE : usize = core::mem::size_of::<AllocationBitmap>();
#[cfg(not(feature = "tlsf-diagnostics"))]
const BITMAP_SIZE : usize = 0;
const OBJECT_OFFSET : usize = HEADER_SIZE + BITMAP_SIZE;

#[inline]
fn adjust_live(counter : &AtomicIsize, delta : isize) {
    // Allocator interrupts are disabled, hence each slot has one writer.  An
    // atomic load/store permits lock-free diagnostic snapshots without a
    // cache-line RMW on the allocation fast path.
    let next = counter.load(Ordering::Relaxed).saturating_add(delta);
    counter.store(next, Ordering::Relaxed);
}

struct Magazine {
    len : usize,
    slots : [usize; 128],
    live_delta : AtomicIsize,
}

impl Magazine {
    const fn new() -> Self {
        Self { len : 0, slots : [0; 128], live_delta : AtomicIsize::new(0) }
    }
}

struct Magazines([Magazine; 8]);

impl Magazines {
    const fn new() -> Self { Self([const { Magazine::new() }; 8]) }
}

// Only the current CPU accesses its slot while allocator interrupts are disabled.
unsafe impl Sync for Magazines {}

static MAGAZINES : CpuLocal<Magazines, MAX_CPUS> =
    CpuLocal::from_cells([const { UnsafeCell::new(Magazines::new()) }; MAX_CPUS]);

struct Central {
    head : usize,
    empty_span : usize,
    spans : usize,
}

impl Central {
    const fn new() -> Self { Self { head : 0, empty_span : 0, spans : 0 } }
}

static CENTRAL : [Mutex<Central>; 8] = [const { Mutex::new(Central::new()) }; 8];

#[inline]
pub(crate) fn class_for(layout : Layout) -> Option<usize> {
    if layout.size() == 0 {
        return None;
    }
    let needed = layout.size().max(layout.align());
    if needed > 2048 {
        return None;
    }
    let rounded = needed.max(16).next_power_of_two();
    Some(rounded.trailing_zeros() as usize - 4)
}

#[inline]
unsafe fn header_for(ptr : *mut u8) -> Option<&'static SpanHeader> {
    if ptr.is_null() {
        return None;
    }
    let base = (ptr as usize) & !(SPAN_SIZE - 1);
    let header = unsafe { &*(base as *const SpanHeader) };
    (header.magic == MAGIC && header.class < CLASS_SIZES.len()).then_some(header)
}

pub(crate) unsafe fn allocation_class(ptr : *mut u8) -> Option<usize> {
    let header = unsafe { header_for(ptr)? };
    let class_size = CLASS_SIZES[header.class];
    let first = (ptr as usize & !(SPAN_SIZE - 1)) + OBJECT_OFFSET;
    let value = ptr as usize;
    if value < first || value >= first + header.capacity * class_size ||
       (value - first) % class_size != 0
    {
        return None;
    }
    Some(header.class)
}

#[inline]
unsafe fn next(object : usize) -> usize { unsafe { *(object as *const usize) } }

#[inline]
unsafe fn set_next(object : usize, value : usize) { unsafe { *(object as *mut usize) = value } }

unsafe fn initialize_span(base : *mut u8, class : usize) {
    let size = CLASS_SIZES[class];
    let capacity = (SPAN_SIZE - OBJECT_OFFSET) / size;
    unsafe {
        ptr::write(base as *mut SpanHeader,
                   SpanHeader { magic : MAGIC,
                                class,
                                capacity,
                                central : AtomicUsize::new(capacity),
                                home_cpu : arch::cpu::current_cpu_id().raw(),
                                next_span : 0,
                                _reserved : [0; 2] });
    }
    #[cfg(feature = "tlsf-diagnostics")]
    unsafe {
        ptr::write(base.add(HEADER_SIZE) as *mut AllocationBitmap,
                   AllocationBitmap { words : [const { AtomicUsize::new(0) }; BITMAP_WORDS] });
    }
    let mut central = CENTRAL[class].lock();
    unsafe { (*(base as *mut SpanHeader)).next_span = central.spans };
    central.spans = base as usize;
    for index in 0..capacity {
        let object = base as usize + OBJECT_OFFSET + index * size;
        unsafe { set_next(object, central.head) };
        central.head = object;
    }
}

fn local_magazine(class : usize) -> &'static mut Magazine {
    let cpu = arch::cpu::current_cpu_id();
    let magazines = unsafe {
        MAGAZINES.get_local_mut(cpu).expect("slab: CPU id exceeds MAX_CPUS")
    };
    &mut magazines.0[class]
}

unsafe fn refill(class : usize, allocate_span : &mut impl FnMut(Layout) -> *mut u8) -> bool {
    let batch = MAG_CAPACITY[class] / 2;
    loop {
        {
            let magazine = local_magazine(class);
            let mut central = CENTRAL[class].lock();
            #[cfg(feature = "tlsf-diagnostics")]
            let before = magazine.len;
            let mut counted_span = 0usize;
            let mut counted_objects = 0usize;
            while magazine.len < batch && central.head != 0 {
                let object = central.head;
                central.head = unsafe { next(object) };
                #[cfg(feature = "tlsf-diagnostics")]
                let _ = unsafe { header_for(object as *mut u8).expect("central slab object") };
                let span = object & !(SPAN_SIZE - 1);
                if counted_span != 0 && counted_span != span {
                    unsafe { &*(counted_span as *const SpanHeader) }
                        .central
                        .fetch_sub(counted_objects, Ordering::Relaxed);
                    counted_objects = 0;
                }
                counted_span = span;
                counted_objects += 1;
                if central.empty_span == span {
                    central.empty_span = 0;
                }
                magazine.slots[magazine.len] = object;
                magazine.len += 1;
            }
            if counted_span != 0 {
                unsafe { &*(counted_span as *const SpanHeader) }
                    .central
                    .fetch_sub(counted_objects, Ordering::Relaxed);
            }
            #[cfg(feature = "tlsf-diagnostics")]
            crate::tlsf_diagnostics::slab_refill(class, magazine.len - before);
            if magazine.len != 0 {
                return true;
            }
        }
        let layout = unsafe { Layout::from_size_align_unchecked(SPAN_SIZE, SPAN_SIZE) };
        let span = allocate_span(layout);
        if span.is_null() {
            return false;
        }
        unsafe { initialize_span(span, class) };
    }
}

#[inline]
unsafe fn activate_object(class : usize, object : usize, local_hit : bool) -> *mut u8 {
    #[cfg(feature = "tlsf-diagnostics")]
    if local_hit {
        crate::tlsf_diagnostics::slab_local_hit(class);
    }
    #[cfg(not(feature = "tlsf-diagnostics"))]
    let _ = local_hit;
    let span = object & !(SPAN_SIZE - 1);
    let index = (object - (span + OBJECT_OFFSET)) / CLASS_SIZES[class];
    #[cfg(feature = "tlsf-diagnostics")]
    {
        let bitmap = unsafe { &*((span + HEADER_SIZE) as *const AllocationBitmap) };
        let mask = 1usize << (index % usize::BITS as usize);
        let previous = bitmap.words[index / usize::BITS as usize].fetch_or(mask, Ordering::AcqRel);
        assert_eq!(previous & mask, 0, "slab object already allocated");
    }
    #[cfg(not(feature = "tlsf-diagnostics"))]
    let _ = index;
    object as *mut u8
}

pub(crate) unsafe fn alloc(layout : Layout,
                           mut allocate_span : impl FnMut(Layout) -> *mut u8)
                           -> *mut u8 {
    let Some(class) = class_for(layout) else {
        return ptr::null_mut();
    };
    let magazine = local_magazine(class);
    if magazine.len != 0 {
        magazine.len -= 1;
        let object = magazine.slots[magazine.len];
        adjust_live(&magazine.live_delta, 1);
        return unsafe { activate_object(class, object, true) };
    }
    if !unsafe { refill(class, &mut allocate_span) } {
        return ptr::null_mut();
    }
    let magazine = local_magazine(class);
    magazine.len -= 1;
    let object = magazine.slots[magazine.len];
    adjust_live(&magazine.live_delta, 1);
    unsafe { activate_object(class, object, false) }
}

unsafe fn remove_span_objects(central : &mut Central, span : usize) {
    let mut previous = 0usize;
    let mut cursor = central.head;
    while cursor != 0 {
        let following = unsafe { next(cursor) };
        if cursor & !(SPAN_SIZE - 1) == span {
            if previous == 0 {
                central.head = following;
            } else {
                unsafe { set_next(previous, following) };
            }
        } else {
            previous = cursor;
        }
        cursor = following;
    }
}

unsafe fn remove_span(central : &mut Central, span : usize) {
    let mut previous = 0usize;
    let mut cursor = central.spans;
    while cursor != 0 {
        let following = unsafe { (*(cursor as *const SpanHeader)).next_span };
        if cursor == span {
            if previous == 0 {
                central.spans = following;
            } else {
                unsafe { (*(previous as *mut SpanHeader)).next_span = following };
            }
            return;
        }
        previous = cursor;
        cursor = following;
    }
}

unsafe fn drain(class : usize,
                count : usize,
                deallocate_span : &mut impl FnMut(*mut u8, Layout)) {
    let magazine = local_magazine(class);
    {
        let mut central = CENTRAL[class].lock();
        let drained = count.min(magazine.len);
        let mut counted_span = 0usize;
        let mut counted_objects = 0usize;
        for _ in 0..drained {
            magazine.len -= 1;
            let object = magazine.slots[magazine.len];
            unsafe { set_next(object, central.head) };
            central.head = object;
            #[cfg(feature = "tlsf-diagnostics")]
            let _ = unsafe { header_for(object as *mut u8).expect("drained slab object") };
            let span = object & !(SPAN_SIZE - 1);
            if counted_span != 0 && counted_span != span {
                unsafe { &*(counted_span as *const SpanHeader) }
                    .central
                    .fetch_add(counted_objects, Ordering::Relaxed);
                counted_objects = 0;
            }
            counted_span = span;
            counted_objects += 1;
        }
        if counted_span != 0 {
            unsafe { &*(counted_span as *const SpanHeader) }
                .central
                .fetch_add(counted_objects, Ordering::Relaxed);
        }
        #[cfg(feature = "tlsf-diagnostics")]
        crate::tlsf_diagnostics::slab_drain(class, drained);
    }

    // One batch may complete several spans. Re-scan until one empty reserve
    // remains, returning each surplus span only after dropping the class lock.
    loop {
        let reclaim = {
            let mut central = CENTRAL[class].lock();
            let mut keep = 0usize;
            let mut reclaim = 0usize;
            let mut span = central.spans;
            while span != 0 {
                let header = unsafe { &*(span as *const SpanHeader) };
                let following = header.next_span;
                if header.central.load(Ordering::Relaxed) == header.capacity
                {
                    if keep == 0 {
                        keep = span;
                    } else {
                        reclaim = span;
                        break;
                    }
                }
                span = following;
            }
            central.empty_span = keep;
            if reclaim != 0 {
                unsafe { remove_span_objects(&mut central, reclaim) };
                unsafe { remove_span(&mut central, reclaim) };
            }
            reclaim
        };
        if reclaim == 0 {
            break;
        }
        unsafe { (*(reclaim as *mut SpanHeader)).magic = 0 };
        let layout = unsafe { Layout::from_size_align_unchecked(SPAN_SIZE, SPAN_SIZE) };
        deallocate_span(reclaim as *mut u8, layout);
    }
}

/// Return the current CPU's cached objects to the central lists.  This is
/// allocation-free and may release surplus completely empty spans before a
/// caller retries TLSF once under memory pressure.
pub(crate) unsafe fn drain_local_all(mut deallocate_span : impl FnMut(*mut u8, Layout)) {
    for class in 0..CLASS_SIZES.len() {
        let count = local_magazine(class).len;
        if count != 0 {
            unsafe { drain(class, count, &mut deallocate_span) };
        }
        let reclaim = {
            let mut central = CENTRAL[class].lock();
            let span = central.empty_span;
            if span == 0 {
                0
            } else {
                let header = unsafe { &*(span as *const SpanHeader) };
                if header.central.load(Ordering::Relaxed) == header.capacity
                {
                    unsafe { remove_span_objects(&mut central, span) };
                    unsafe { remove_span(&mut central, span) };
                    central.empty_span = 0;
                    span
                } else {
                    central.empty_span = 0;
                    0
                }
            }
        };
        if reclaim != 0 {
            unsafe { (*(reclaim as *mut SpanHeader)).magic = 0 };
            let layout = unsafe { Layout::from_size_align_unchecked(SPAN_SIZE, SPAN_SIZE) };
            deallocate_span(reclaim as *mut u8, layout);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SlabMemStats {
    pub application_live : usize,
    pub per_cpu_cached : usize,
    pub central_free : usize,
    pub tlsf_owned : usize,
}

/// Derive the accounting split from per-span counters.  Allocation/free fast
/// paths therefore need no global accounting atomic RMW.
pub(crate) fn mem_stats() -> SlabMemStats {
    let mut result = SlabMemStats::default();
    let mut live_objects = [0isize; 8];
    for cpu in 0..MAX_CPUS {
        let counters = MAGAZINES.get(base::cpu::CpuId::from_raw(cpu))
                                .expect("valid slab CPU slot");
        for (class, total) in live_objects.iter_mut().enumerate() {
            *total = total.saturating_add(
                counters.0[class].live_delta.load(Ordering::Relaxed),
            );
        }
    }
    for class in 0..CLASS_SIZES.len() {
        let central = CENTRAL[class].lock();
        let mut object_capacity = 0usize;
        let mut central_objects = 0usize;
        let mut span = central.spans;
        while span != 0 {
            let header = unsafe { &*(span as *const SpanHeader) };
            let central_free = header.central.load(Ordering::Relaxed);
            object_capacity = object_capacity.saturating_add(header.capacity);
            central_objects = central_objects.saturating_add(central_free);
            result.tlsf_owned = result.tlsf_owned.saturating_add(SPAN_SIZE);
            span = header.next_span;
        }
        let allocated = usize::try_from(live_objects[class].max(0)).unwrap_or(usize::MAX);
        let noncentral = object_capacity.saturating_sub(central_objects);
        result.application_live = result.application_live
                                        .saturating_add(allocated * CLASS_SIZES[class]);
        result.central_free = result.central_free
                                    .saturating_add(central_objects * CLASS_SIZES[class]);
        result.per_cpu_cached = result.per_cpu_cached
                                      .saturating_add(noncentral.saturating_sub(allocated) *
                                                      CLASS_SIZES[class]);
    }
    result
}

pub(crate) unsafe fn dealloc(ptr : *mut u8,
                             layout : Layout,
                             mut deallocate_span : impl FnMut(*mut u8, Layout))
                             -> bool {
    let Some(class) = (unsafe { allocation_class(ptr) }) else {
        return false;
    };
    if class_for(layout) != Some(class) {
        // It is definitely a slab pointer. Never pass it to TLSF with a
        // mismatched layout, which would corrupt TLSF metadata.
        #[cfg(feature = "tlsf-diagnostics")]
        panic!("slab dealloc layout mismatch ptr={ptr:p} size={} align={}",
               layout.size(),
               layout.align());
        #[cfg(not(feature = "tlsf-diagnostics"))]
        return true;
    }
    let header = unsafe { header_for(ptr).expect("validated slab object") };
    let span = ptr as usize & !(SPAN_SIZE - 1);
    let index = (ptr as usize - (span + OBJECT_OFFSET)) / CLASS_SIZES[class];
    #[cfg(feature = "tlsf-diagnostics")]
    {
        let bitmap = unsafe { &*((span + HEADER_SIZE) as *const AllocationBitmap) };
        let mask = 1usize << (index % usize::BITS as usize);
        let previous = bitmap.words[index / usize::BITS as usize].fetch_and(!mask, Ordering::AcqRel);
        if previous & mask == 0 {
        panic!("slab double free ptr={ptr:p} class={class}");
        }
    }
    #[cfg(not(feature = "tlsf-diagnostics"))]
    let _ = index;
    #[cfg(feature = "tlsf-diagnostics")]
    if header.home_cpu != arch::cpu::current_cpu_id().raw() {
        crate::tlsf_diagnostics::slab_cross_cpu_free(class);
    }
    let capacity = MAG_CAPACITY[class];
    let magazine = local_magazine(class);
    adjust_live(&magazine.live_delta, -1);
    if magazine.len < capacity {
        magazine.slots[magazine.len] = ptr as usize;
        magazine.len += 1;
        return true;
    }
    unsafe { drain(class, capacity / 2, &mut deallocate_span) };
    let magazine = local_magazine(class);
    magazine.slots[magazine.len] = ptr as usize;
    magazine.len += 1;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_selection_honors_size_and_alignment_boundaries() {
        for (index, size) in CLASS_SIZES.iter().copied().enumerate() {
            assert_eq!(class_for(Layout::from_size_align(size, 1).unwrap()), Some(index));
            assert_eq!(class_for(Layout::from_size_align(1, size).unwrap()), Some(index));
        }
        assert_eq!(class_for(Layout::from_size_align(17, 16).unwrap()), Some(1));
        assert_eq!(class_for(Layout::from_size_align(2049, 1).unwrap()), None);
        assert_eq!(class_for(Layout::from_size_align(1, 4096).unwrap()), None);
        assert_eq!(class_for(Layout::from_size_align(0, 1).unwrap()), None);
    }
}
