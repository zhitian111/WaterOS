//! Feature-gated per-CPU slab diagnostics.

use core::sync::atomic::{AtomicU64, Ordering};

use super::size_class::SIZE_CLASS_COUNT;

#[derive(Clone, Copy, Default)]
pub(crate) struct SlabDiagnosticsSnapshot {
    pub(crate) allocs : [u64; SIZE_CLASS_COUNT],
    pub(crate) local_hits : [u64; SIZE_CLASS_COUNT],
    pub(crate) local_frees : [u64; SIZE_CLASS_COUNT],
    pub(crate) remote_frees : [u64; SIZE_CLASS_COUNT],
    pub(crate) frame_refills : [u64; SIZE_CLASS_COUNT],
    pub(crate) frame_reclaims : [u64; SIZE_CLASS_COUNT],
    pub(crate) pages : [u64; SIZE_CLASS_COUNT],
    pub(crate) empty_pages : [u64; SIZE_CLASS_COUNT],
    pub(crate) fallbacks : u64,
    pub(crate) oom : u64,
    pub(crate) remote_cas_misses : u64,
    pub(crate) drain_events : u64,
    pub(crate) drain_objects : u64,
    pub(crate) drain_max : u64,
}

impl SlabDiagnosticsSnapshot {
    pub(crate) fn is_empty(&self) -> bool {
        self.allocs
            .iter()
            .all(|value| *value == 0) &&
        self.local_frees
            .iter()
            .all(|value| *value == 0) &&
        self.remote_frees
            .iter()
            .all(|value| *value == 0) &&
        self.fallbacks == 0 &&
        self.oom == 0 &&
        self.remote_cas_misses == 0 &&
        self.drain_events == 0
    }

    pub(crate) fn add_assign(&mut self, other : &Self) {
        for idx in 0..SIZE_CLASS_COUNT {
            self.allocs[idx] = self.allocs[idx].saturating_add(other.allocs[idx]);
            self.local_hits[idx] = self.local_hits[idx].saturating_add(other.local_hits[idx]);
            self.local_frees[idx] = self.local_frees[idx].saturating_add(other.local_frees[idx]);
            self.remote_frees[idx] = self.remote_frees[idx].saturating_add(other.remote_frees[idx]);
            self.frame_refills[idx] =
                self.frame_refills[idx].saturating_add(other.frame_refills[idx]);
            self.frame_reclaims[idx] =
                self.frame_reclaims[idx].saturating_add(other.frame_reclaims[idx]);
            self.pages[idx] = self.pages[idx].saturating_add(other.pages[idx]);
            self.empty_pages[idx] = self.empty_pages[idx].saturating_add(other.empty_pages[idx]);
        }
        self.fallbacks = self.fallbacks
                             .saturating_add(other.fallbacks);
        self.oom = self.oom
                       .saturating_add(other.oom);
        self.remote_cas_misses = self.remote_cas_misses
                                     .saturating_add(other.remote_cas_misses);
        self.drain_events = self.drain_events
                                .saturating_add(other.drain_events);
        self.drain_objects = self.drain_objects
                                 .saturating_add(other.drain_objects);
        self.drain_max = self.drain_max
                             .max(other.drain_max);
    }
}

/// One cache-line-aligned slot per CPU. The owner CPU is the only writer and holds the allocator
/// interrupt guard, so counters use load/store rather than contended read-modify-write operations.
#[repr(align(64))]
pub(crate) struct SlabDiagnostics {
    allocs : [AtomicU64; SIZE_CLASS_COUNT],
    local_hits : [AtomicU64; SIZE_CLASS_COUNT],
    local_frees : [AtomicU64; SIZE_CLASS_COUNT],
    remote_frees : [AtomicU64; SIZE_CLASS_COUNT],
    frame_refills : [AtomicU64; SIZE_CLASS_COUNT],
    frame_reclaims : [AtomicU64; SIZE_CLASS_COUNT],
    pages : [AtomicU64; SIZE_CLASS_COUNT],
    empty_pages : [AtomicU64; SIZE_CLASS_COUNT],
    fallbacks : AtomicU64,
    oom : AtomicU64,
    remote_cas_misses : AtomicU64,
    drain_events : AtomicU64,
    drain_objects : AtomicU64,
    drain_max : AtomicU64,
}

impl SlabDiagnostics {
    pub(crate) fn new() -> Self {
        Self { allocs : core::array::from_fn(|_| AtomicU64::new(0)),
               local_hits : core::array::from_fn(|_| AtomicU64::new(0)),
               local_frees : core::array::from_fn(|_| AtomicU64::new(0)),
               remote_frees : core::array::from_fn(|_| AtomicU64::new(0)),
               frame_refills : core::array::from_fn(|_| AtomicU64::new(0)),
               frame_reclaims : core::array::from_fn(|_| AtomicU64::new(0)),
               pages : core::array::from_fn(|_| AtomicU64::new(0)),
               empty_pages : core::array::from_fn(|_| AtomicU64::new(0)),
               fallbacks : AtomicU64::new(0),
               oom : AtomicU64::new(0),
               remote_cas_misses : AtomicU64::new(0),
               drain_events : AtomicU64::new(0),
               drain_objects : AtomicU64::new(0),
               drain_max : AtomicU64::new(0) }
    }

    #[inline]
    fn increment(counter : &AtomicU64) {
        let current = counter.load(Ordering::Relaxed);
        counter.store(current.saturating_add(1),
                      Ordering::Relaxed);
    }

    #[inline]
    fn add(counter : &AtomicU64, value : usize) {
        let current = counter.load(Ordering::Relaxed);
        counter.store(current.saturating_add(value as u64),
                      Ordering::Relaxed);
    }

    pub(crate) fn record_alloc(&self, class : usize) { Self::increment(&self.allocs[class]); }

    pub(crate) fn record_local_hit(&self, class : usize) {
        Self::increment(&self.local_hits[class]);
    }

    pub(crate) fn record_local_free(&self, class : usize) {
        Self::increment(&self.local_frees[class]);
    }

    pub(crate) fn record_remote_free(&self, class : usize, cas_misses : usize) {
        Self::increment(&self.remote_frees[class]);
        Self::add(&self.remote_cas_misses, cas_misses);
    }

    pub(crate) fn record_frame_refill(&self, class : usize) {
        Self::increment(&self.frame_refills[class]);
        Self::increment(&self.pages[class]);
        Self::increment(&self.empty_pages[class]);
    }

    pub(crate) fn record_frame_reclaim(&self, class : usize) {
        Self::increment(&self.frame_reclaims[class]);
        let pages = self.pages[class].load(Ordering::Relaxed);
        self.pages[class].store(pages.saturating_sub(1), Ordering::Relaxed);
        let empty_pages = self.empty_pages[class].load(Ordering::Relaxed);
        self.empty_pages[class].store(empty_pages.saturating_sub(1), Ordering::Relaxed);
    }

    pub(crate) fn record_page_became_empty(&self, class : usize) {
        Self::increment(&self.empty_pages[class]);
    }

    pub(crate) fn record_page_became_nonempty(&self, class : usize) {
        let current = self.empty_pages[class].load(Ordering::Relaxed);
        self.empty_pages[class].store(current.saturating_sub(1),
                                      Ordering::Relaxed);
    }

    pub(crate) fn record_fallback(&self) { Self::increment(&self.fallbacks); }

    pub(crate) fn record_oom(&self) { Self::increment(&self.oom); }

    pub(crate) fn record_drain(&self, objects : usize) {
        Self::increment(&self.drain_events);
        Self::add(&self.drain_objects, objects);
        let current = self.drain_max
                          .load(Ordering::Relaxed);
        if objects as u64 > current {
            self.drain_max
                .store(objects as u64, Ordering::Relaxed);
        }
    }

    pub(crate) fn snapshot(&self) -> SlabDiagnosticsSnapshot {
        let load_array = |source : &[AtomicU64; SIZE_CLASS_COUNT]| {
            core::array::from_fn(|idx| source[idx].load(Ordering::Relaxed))
        };
        SlabDiagnosticsSnapshot { allocs : load_array(&self.allocs),
                                  local_hits : load_array(&self.local_hits),
                                  local_frees : load_array(&self.local_frees),
                                  remote_frees : load_array(&self.remote_frees),
                                  frame_refills : load_array(&self.frame_refills),
                                  frame_reclaims : load_array(&self.frame_reclaims),
                                  pages : load_array(&self.pages),
                                  empty_pages : load_array(&self.empty_pages),
                                  fallbacks : self.fallbacks
                                                  .load(Ordering::Relaxed),
                                  oom : self.oom
                                            .load(Ordering::Relaxed),
                                  remote_cas_misses : self.remote_cas_misses
                                                          .load(Ordering::Relaxed),
                                  drain_events : self.drain_events
                                                     .load(Ordering::Relaxed),
                                  drain_objects : self.drain_objects
                                                      .load(Ordering::Relaxed),
                                  drain_max : self.drain_max
                                                  .load(Ordering::Relaxed) }
    }
}
