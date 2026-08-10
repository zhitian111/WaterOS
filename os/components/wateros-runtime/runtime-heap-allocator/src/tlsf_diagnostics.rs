//! Allocation-path counters used by the BuildStorm profiling build.
//!
//! This module is only compiled with `tlsf-diagnostics`; the normal/final
//! allocator contains neither the counters nor their atomic operations.

use core::alloc::Layout;
use core::array;
use core::sync::atomic::{AtomicUsize, Ordering};

use base::cpu::CpuLocal;
use config::task::MAX_CPUS;

pub(crate) const CLASS_COUNT : usize = 9;
pub const SAMPLE_RATE : usize = 64;
pub const CLASS_LABELS : [&str; CLASS_COUNT] =
    ["16", "32", "64", "128", "256", "512", "1024", "2048", "gt2048"];

struct ClassCounters {
    alloc : AtomicUsize,
    free : AtomicUsize,
    realloc : AtomicUsize,
    over_aligned : AtomicUsize,
    max_alignment : AtomicUsize,
    live_objects : AtomicUsize,
    live_bytes : AtomicUsize,
    peak_live_objects : AtomicUsize,
    peak_live_bytes : AtomicUsize,
    local_hit : AtomicUsize,
    refill : AtomicUsize,
    drain : AtomicUsize,
    cross_cpu_free : AtomicUsize,
}

impl ClassCounters {
    const fn new() -> Self {
        Self { alloc : AtomicUsize::new(0),
               free : AtomicUsize::new(0),
               realloc : AtomicUsize::new(0),
               over_aligned : AtomicUsize::new(0),
               max_alignment : AtomicUsize::new(0),
               live_objects : AtomicUsize::new(0),
               live_bytes : AtomicUsize::new(0),
               peak_live_objects : AtomicUsize::new(0),
               peak_live_bytes : AtomicUsize::new(0),
               local_hit : AtomicUsize::new(0),
               refill : AtomicUsize::new(0),
               drain : AtomicUsize::new(0),
               cross_cpu_free : AtomicUsize::new(0) }
    }
}

struct CpuCounters {
    classes : [ClassCounters; CLASS_COUNT],
    tlsf_lock_acquire : AtomicUsize,
    tlsf_lock_spin : AtomicUsize,
    tlsf_fallback : AtomicUsize,
    oom : AtomicUsize,
    event_seq : AtomicUsize,
    lock_seq : AtomicUsize,
}

impl CpuCounters {
    const fn new() -> Self {
        Self { classes : [const { ClassCounters::new() }; CLASS_COUNT],
               tlsf_lock_acquire : AtomicUsize::new(0),
               tlsf_lock_spin : AtomicUsize::new(0),
               tlsf_fallback : AtomicUsize::new(0),
               oom : AtomicUsize::new(0),
               event_seq : AtomicUsize::new(0),
               lock_seq : AtomicUsize::new(0) }
    }
}

static COUNTERS : CpuLocal<CpuCounters, MAX_CPUS> =
    CpuLocal::from_cells([const { core::cell::UnsafeCell::new(CpuCounters::new()) }; MAX_CPUS]);

#[inline]
pub(crate) const fn class_index(size : usize) -> usize {
    if size <= 16 { 0 } else if size <= 32 { 1 } else if size <= 64 { 2 } else if size <= 128 {
        3
    } else if size <= 256 {
        4
    } else if size <= 512 {
        5
    } else if size <= 1024 {
        6
    } else if size <= 2048 {
        7
    } else {
        8
    }
}

#[inline]
fn local() -> &'static CpuCounters {
    COUNTERS.get(arch::cpu::current_cpu_id()).expect("TLSF diagnostics: invalid CPU id")
}

#[inline]
fn add_local(counter : &AtomicUsize, value : usize) -> usize {
    // Allocator interrupts are disabled, so each CpuCounters slot has exactly
    // one writer. Atomic load/store keeps concurrent diagnostic snapshots
    // data-race-free without paying for a locked RMW instruction.
    let next = counter.load(Ordering::Relaxed).saturating_add(value);
    counter.store(next, Ordering::Relaxed);
    next
}

#[inline]
fn sub_local(counter : &AtomicUsize, value : usize) {
    let next = counter.load(Ordering::Relaxed).saturating_sub(value);
    counter.store(next, Ordering::Relaxed);
}

fn update_peak(peak : &AtomicUsize, value : usize) {
    if value > peak.load(Ordering::Relaxed) {
        peak.store(value, Ordering::Relaxed);
    }
}

#[inline]
fn sample(counter : &AtomicUsize) -> bool {
    add_local(counter, 1) % SAMPLE_RATE == 0
}

pub(crate) fn alloc(layout : Layout, success : bool, tlsf_fallback : bool) {
    let local = local();
    if !sample(&local.event_seq) {
        if !success {
            add_local(&local.oom, 1);
        }
        return;
    }
    let class = &local.classes[class_index(layout.size())];
    add_local(&class.alloc, SAMPLE_RATE);
    if layout.align() > layout.size().next_power_of_two() {
        add_local(&class.over_aligned, SAMPLE_RATE);
    }
    update_peak(&class.max_alignment, layout.align());
    if tlsf_fallback {
        add_local(&local.tlsf_fallback, SAMPLE_RATE);
    }
    if !success {
        add_local(&local.oom, 1);
        return;
    }
    let objects = add_local(&class.live_objects, SAMPLE_RATE);
    let bytes = add_local(&class.live_bytes, layout.size().saturating_mul(SAMPLE_RATE));
    update_peak(&class.peak_live_objects, objects);
    update_peak(&class.peak_live_bytes, bytes);
}

pub(crate) fn free(layout : Layout) {
    let local = local();
    if !sample(&local.event_seq) {
        return;
    }
    let class = &local.classes[class_index(layout.size())];
    add_local(&class.free, SAMPLE_RATE);
    sub_local(&class.live_objects, SAMPLE_RATE);
    sub_local(&class.live_bytes, layout.size().saturating_mul(SAMPLE_RATE));
}

pub(crate) fn realloc(old : Layout, new_size : usize, success : bool) {
    let local = local();
    if !sample(&local.event_seq) {
        if !success {
            add_local(&local.oom, 1);
        }
        return;
    }
    let old_class = &local.classes[class_index(old.size())];
    add_local(&old_class.realloc, SAMPLE_RATE);
    if success {
        add_local(&old_class.free, SAMPLE_RATE);
        sub_local(&old_class.live_objects, SAMPLE_RATE);
        sub_local(&old_class.live_bytes, old.size().saturating_mul(SAMPLE_RATE));
        if let Ok(new) = Layout::from_size_align(new_size, old.align()) {
            let new_class = &local.classes[class_index(new.size())];
            add_local(&new_class.alloc, SAMPLE_RATE);
            update_peak(&new_class.max_alignment, new.align());
            add_local(&local.tlsf_fallback, SAMPLE_RATE);
            let objects = add_local(&new_class.live_objects, SAMPLE_RATE);
            let bytes = add_local(&new_class.live_bytes,
                                  new.size().saturating_mul(SAMPLE_RATE));
            update_peak(&new_class.peak_live_objects, objects);
            update_peak(&new_class.peak_live_bytes, bytes);
        }
    } else {
        add_local(&local.oom, 1);
    }
}

/// Record a slab realloc whose alloc/free accounting was already performed by
/// the nested allocator operations (or which stayed in the same class).
pub(crate) fn slab_realloc(old : Layout, success : bool) {
    let local = local();
    if sample(&local.event_seq) {
        add_local(&local.classes[class_index(old.size())].realloc, SAMPLE_RATE);
    }
    if !success {
        add_local(&local.oom, 1);
    }
}

pub(crate) fn lock(acquired_after_spin : bool) {
    let local = local();
    if !sample(&local.lock_seq) {
        return;
    }
    add_local(&local.tlsf_lock_acquire, SAMPLE_RATE);
    if acquired_after_spin {
        add_local(&local.tlsf_lock_spin, SAMPLE_RATE);
    }
}

pub(crate) fn slab_local_hit(class : usize) {
    add_local(&local().classes[class].local_hit, 1);
}

pub(crate) fn slab_refill(class : usize, count : usize) {
    add_local(&local().classes[class].refill, count);
}

pub(crate) fn slab_drain(class : usize, count : usize) {
    add_local(&local().classes[class].drain, count);
}

pub(crate) fn slab_cross_cpu_free(class : usize) {
    add_local(&local().classes[class].cross_cpu_free, 1);
}

/// Aggregated diagnostic snapshot. Reading it performs no allocation.
#[derive(Clone, Copy, Debug, Default)]
pub struct TlsfDiagnosticClass {
    pub alloc : usize,
    pub free : usize,
    pub realloc : usize,
    pub over_aligned : usize,
    pub max_alignment : usize,
    pub live_objects : usize,
    pub live_bytes : usize,
    pub peak_live_objects : usize,
    pub peak_live_bytes : usize,
    pub local_hit : usize,
    pub refill : usize,
    pub drain : usize,
    pub cross_cpu_free : usize,
}

#[derive(Clone, Debug)]
pub struct TlsfDiagnostics {
    pub classes : [TlsfDiagnosticClass; CLASS_COUNT],
    pub tlsf_lock_acquire : usize,
    pub tlsf_lock_spin : usize,
    pub tlsf_fallback : usize,
    pub oom : usize,
    pub sampling_rate : usize,
}

pub fn snapshot() -> TlsfDiagnostics {
    let mut result = TlsfDiagnostics { classes : array::from_fn(|_| TlsfDiagnosticClass::default()),
                                       tlsf_lock_acquire : 0,
                                       tlsf_lock_spin : 0,
                                       tlsf_fallback : 0,
                                       oom : 0,
                                       sampling_rate : SAMPLE_RATE };
    for cpu in 0..MAX_CPUS {
        let c = COUNTERS.get(base::cpu::CpuId::from_raw(cpu)).expect("valid CPU slot");
        result.tlsf_lock_acquire += c.tlsf_lock_acquire.load(Ordering::Relaxed);
        result.tlsf_lock_spin += c.tlsf_lock_spin.load(Ordering::Relaxed);
        result.tlsf_fallback += c.tlsf_fallback.load(Ordering::Relaxed);
        result.oom += c.oom.load(Ordering::Relaxed);
        for (dst, src) in result.classes.iter_mut().zip(c.classes.iter()) {
            dst.alloc += src.alloc.load(Ordering::Relaxed);
            dst.free += src.free.load(Ordering::Relaxed);
            dst.realloc += src.realloc.load(Ordering::Relaxed);
            dst.over_aligned += src.over_aligned.load(Ordering::Relaxed);
            dst.max_alignment = dst.max_alignment.max(src.max_alignment.load(Ordering::Relaxed));
            dst.live_objects += src.live_objects.load(Ordering::Relaxed);
            dst.live_bytes += src.live_bytes.load(Ordering::Relaxed);
            dst.peak_live_objects += src.peak_live_objects.load(Ordering::Relaxed);
            dst.peak_live_bytes += src.peak_live_bytes.load(Ordering::Relaxed);
            dst.local_hit += src.local_hit.load(Ordering::Relaxed);
            dst.refill += src.refill.load(Ordering::Relaxed);
            dst.drain += src.drain.load(Ordering::Relaxed);
            dst.cross_cpu_free += src.cross_cpu_free.load(Ordering::Relaxed);
        }
    }
    result
}

/// Emit runner-compatible counters without constructing a temporary string.
pub fn emit_buildstorm_counters() {
    let stats = snapshot();
    log::error!("BUILDSTORM_PERF_COUNTERS tlsf_sampling_rate={} tlsf_lock_acquire={} tlsf_lock_spin={} tlsf_fallback={} tlsf_oom={}",
                stats.sampling_rate,
                stats.tlsf_lock_acquire,
                stats.tlsf_lock_spin,
                stats.tlsf_fallback,
                stats.oom);
    macro_rules! emit_class {
        ($idx:literal, $label:literal) => {{
            let c = &stats.classes[$idx];
            log::error!(concat!("BUILDSTORM_PERF_COUNTERS tlsf_", $label, "_alloc={} tlsf_",
                                $label, "_free={} tlsf_", $label, "_realloc={} tlsf_",
                                $label, "_over_aligned={} tlsf_", $label,
                                "_max_alignment={} tlsf_", $label,
                                "_live_objects={} tlsf_", $label, "_live_bytes={} tlsf_",
                                $label, "_peak_objects={} tlsf_", $label, "_peak_bytes={} tlsf_",
                                $label, "_local_hit={} tlsf_", $label, "_refill={} tlsf_",
                                $label, "_drain={} tlsf_", $label, "_cross_cpu_free={}"),
                        c.alloc, c.free, c.realloc, c.over_aligned, c.max_alignment,
                        c.live_objects, c.live_bytes,
                        c.peak_live_objects, c.peak_live_bytes, c.local_hit, c.refill, c.drain,
                        c.cross_cpu_free);
        }};
    }
    emit_class!(0, "16");
    emit_class!(1, "32");
    emit_class!(2, "64");
    emit_class!(3, "128");
    emit_class!(4, "256");
    emit_class!(5, "512");
    emit_class!(6, "1024");
    emit_class!(7, "2048");
    emit_class!(8, "gt2048");
}
