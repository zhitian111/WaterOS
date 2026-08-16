//! 低频 slab 页状态统计。
//!
//! retained page 只在 refill/reclaim 时更新；reclaimable page 只统计脱离 current 的
//! warm reserve。current 页反复变空/再激活不会写统计，避免把观测开销带回对象热路径。

use core::sync::atomic::{AtomicUsize, Ordering};

use super::size_class::SIZE_CLASS_COUNT;

#[derive(Clone, Copy, Default)]
pub(crate) struct SlabPageMemStats {
    pub(crate) pages : usize,
    pub(crate) reclaimable_pages : usize,
}

#[repr(align(64))]
pub(crate) struct SlabPageStats {
    pages : [AtomicUsize; SIZE_CLASS_COUNT],
    reclaimable_pages : [AtomicUsize; SIZE_CLASS_COUNT],
}

impl SlabPageStats {
    pub(crate) fn new() -> Self {
        Self { pages : core::array::from_fn(|_| AtomicUsize::new(0)),
               reclaimable_pages : core::array::from_fn(|_| AtomicUsize::new(0)) }
    }

    #[inline]
    fn increment(counter : &AtomicUsize) {
        let current = counter.load(Ordering::Relaxed);
        counter.store(current.saturating_add(1),
                      Ordering::Relaxed);
    }

    #[inline]
    fn decrement(counter : &AtomicUsize) {
        let current = counter.load(Ordering::Relaxed);
        counter.store(current.saturating_sub(1),
                      Ordering::Relaxed);
    }

    pub(crate) fn record_refill(&self, class : usize) {
        Self::increment(&self.pages[class]);
    }

    pub(crate) fn record_became_nonempty(&self, class : usize) {
        Self::decrement(&self.reclaimable_pages[class]);
    }

    pub(crate) fn record_became_empty(&self, class : usize) {
        Self::increment(&self.reclaimable_pages[class]);
    }

    pub(crate) fn record_reclaim(&self, class : usize) {
        Self::decrement(&self.pages[class]);
    }

    pub(crate) fn snapshot(&self) -> SlabPageMemStats {
        SlabPageMemStats { pages : self.pages
                                       .iter()
                                       .map(|counter| counter.load(Ordering::Relaxed))
                                       .sum(),
                           reclaimable_pages : self.reclaimable_pages
                                                   .iter()
                                                   .map(|counter| {
                                                       counter.load(Ordering::Relaxed)
                                                   })
                                                   .sum() }
    }
}
