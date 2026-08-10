//! Allocation-only diagnostics. This module is absent from ordinary builds.

use core::alloc::Layout;
use core::cell::UnsafeCell;

use base::cpu::{CpuId, CpuLocal};
use config::task::MAX_CPUS;

const CLASS_COUNT : usize = 9;
const CLASS_LIMITS : [usize; CLASS_COUNT - 1] = [16, 32, 64, 128, 256, 512, 1024, 2048];

#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct PerCpuCounters {
    alloc : [u64; CLASS_COUNT],
    free : [u64; CLASS_COUNT],
    realloc : [u64; CLASS_COUNT],
    alloc_bytes : [u64; CLASS_COUNT],
    align_gt16 : u64,
    lock_acquire : u64,
    lock_contended : u64,
    oom : u64,
}

impl PerCpuCounters {
    const fn new() -> Self {
        Self { alloc : [0; CLASS_COUNT],
               free : [0; CLASS_COUNT],
               realloc : [0; CLASS_COUNT],
               alloc_bytes : [0; CLASS_COUNT],
               align_gt16 : 0,
               lock_acquire : 0,
               lock_contended : 0,
               oom : 0 }
    }

    fn add_from(&mut self, other : &Self) {
        for i in 0..CLASS_COUNT {
            self.alloc[i] = self.alloc[i]
                                 .saturating_add(other.alloc[i]);
            self.free[i] = self.free[i]
                               .saturating_add(other.free[i]);
            self.realloc[i] = self.realloc[i]
                                  .saturating_add(other.realloc[i]);
            self.alloc_bytes[i] = self.alloc_bytes[i]
                                      .saturating_add(other.alloc_bytes[i]);
        }
        self.align_gt16 = self.align_gt16
                              .saturating_add(other.align_gt16);
        self.lock_acquire = self.lock_acquire
                                .saturating_add(other.lock_acquire);
        self.lock_contended = self.lock_contended
                                  .saturating_add(other.lock_contended);
        self.oom = self.oom
                       .saturating_add(other.oom);
    }
}

static COUNTERS : CpuLocal<PerCpuCounters, MAX_CPUS> =
    CpuLocal::from_cells([const { UnsafeCell::new(PerCpuCounters::new()) }; MAX_CPUS]);

#[inline]
fn class(size : usize) -> usize {
    CLASS_LIMITS.iter()
                .position(|limit| size <= *limit)
                .unwrap_or(CLASS_COUNT - 1)
}

#[inline]
fn with_current_counters(f : impl FnOnce(&mut PerCpuCounters)) {
    let cpu = arch::cpu::current_cpu_id();
    let Some(counters) = (unsafe { COUNTERS.get_local_mut(cpu) }) else {
        return;
    };
    f(counters);
}

#[inline]
fn record_lock(counters : &mut PerCpuCounters, contended : bool) {
    counters.lock_acquire = counters.lock_acquire
                                    .saturating_add(1);
    if contended {
        counters.lock_contended = counters.lock_contended
                                          .saturating_add(1);
    }
}

pub(crate) fn record_alloc(layout : Layout, success : bool, contended : bool) {
    with_current_counters(|counters| {
        let class = class(layout.size());
        counters.alloc[class] = counters.alloc[class]
                                        .saturating_add(1);
        counters.alloc_bytes[class] = counters.alloc_bytes[class]
                                              .saturating_add(layout.size() as u64);
        if layout.align() > 16 {
            counters.align_gt16 = counters.align_gt16
                                          .saturating_add(1);
        }
        if !success {
            counters.oom = counters.oom
                                   .saturating_add(1);
        }
        record_lock(counters, contended);
    });
}

pub(crate) fn record_free(layout : Layout, contended : bool) {
    with_current_counters(|counters| {
        counters.free[class(layout.size())] =
            counters.free[class(layout.size())]
                    .saturating_add(1);
        record_lock(counters, contended);
    });
}

pub(crate) fn record_realloc(layout : Layout,
                             new_size : usize,
                             success : bool,
                             contended : bool) {
    with_current_counters(|counters| {
        let class = class(layout.size().max(new_size));
        counters.realloc[class] = counters.realloc[class]
                                          .saturating_add(1);
        if !success && new_size != 0 {
            counters.oom = counters.oom
                                   .saturating_add(1);
        }
        record_lock(counters, contended);
    });
}

fn aggregate() -> PerCpuCounters {
    let mut total = PerCpuCounters::new();
    for raw in 0..MAX_CPUS {
        if let Some(counters) = COUNTERS.get(CpuId::from_raw(raw)) {
            total.add_from(counters);
        }
    }
    total
}

pub fn emit_buildstorm_counters() {
    let counters = aggregate();
    log::info!("BUILDSTORM_PERF_COUNTERS tlsf_alloc_16={} tlsf_alloc_32={} tlsf_alloc_64={} tlsf_alloc_128={} tlsf_alloc_256={} tlsf_alloc_512={} tlsf_alloc_1024={} tlsf_alloc_2048={} tlsf_alloc_large={}",
               counters.alloc[0], counters.alloc[1],
               counters.alloc[2], counters.alloc[3],
               counters.alloc[4], counters.alloc[5],
               counters.alloc[6], counters.alloc[7],
               counters.alloc[8]);
    log::info!("BUILDSTORM_PERF_COUNTERS tlsf_free_16={} tlsf_free_32={} tlsf_free_64={} tlsf_free_128={} tlsf_free_256={} tlsf_free_512={} tlsf_free_1024={} tlsf_free_2048={} tlsf_free_large={}",
               counters.free[0], counters.free[1],
               counters.free[2], counters.free[3],
               counters.free[4], counters.free[5],
               counters.free[6], counters.free[7],
               counters.free[8]);
    log::info!("BUILDSTORM_PERF_COUNTERS tlsf_realloc_16={} tlsf_realloc_32={} tlsf_realloc_64={} tlsf_realloc_128={} tlsf_realloc_256={} tlsf_realloc_512={} tlsf_realloc_1024={} tlsf_realloc_2048={} tlsf_realloc_large={}",
               counters.realloc[0], counters.realloc[1],
               counters.realloc[2], counters.realloc[3],
               counters.realloc[4], counters.realloc[5],
               counters.realloc[6], counters.realloc[7],
               counters.realloc[8]);
    log::info!("BUILDSTORM_PERF_COUNTERS tlsf_bytes_16={} tlsf_bytes_32={} tlsf_bytes_64={} tlsf_bytes_128={} tlsf_bytes_256={} tlsf_bytes_512={} tlsf_bytes_1024={} tlsf_bytes_2048={} tlsf_bytes_large={} tlsf_align_gt16={} tlsf_lock_acquire={} tlsf_lock_contended={} tlsf_oom={}",
               counters.alloc_bytes[0], counters.alloc_bytes[1],
               counters.alloc_bytes[2], counters.alloc_bytes[3],
               counters.alloc_bytes[4], counters.alloc_bytes[5],
               counters.alloc_bytes[6], counters.alloc_bytes[7],
               counters.alloc_bytes[8], counters.align_gt16,
               counters.lock_acquire, counters.lock_contended,
               counters.oom);
}
