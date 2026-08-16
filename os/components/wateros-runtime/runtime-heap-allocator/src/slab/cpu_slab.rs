//! 单个 CPU 持有的全部 size class cache。

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use super::page_stats::SlabPageStats;
use super::size_class::SIZE_CLASS_COUNT;
use super::slab_cache::SlabCache;
use super::slab_page::{read_next, write_next, SlabPageHeader, SLAB_MAGIC};
use super::HeapFrameSource;
#[cfg(feature = "slab-diagnostics")]
use super::diagnostics::SlabDiagnostics;

/// 单次 owner drain 的硬上限。定时维护和同 class 分配会继续推进剩余对象。
pub(crate) const REMOTE_DRAIN_LIMIT : usize = 256;

/// 一个 owner CPU 的按 size-class remote-free 队列。
pub(crate) struct RemoteFreeQueues {
    heads : [AtomicPtr<u8>; SIZE_CLASS_COUNT],
    pending_classes : AtomicUsize,
}

impl RemoteFreeQueues {
    pub(crate) fn new() -> Self {
        Self { heads : core::array::from_fn(|_| AtomicPtr::new(core::ptr::null_mut())),
               pending_classes : AtomicUsize::new(0) }
    }

    #[inline]
    pub(crate) fn has_pending(&self, class : usize) -> bool {
        !self.heads[class]
             .load(Ordering::Relaxed)
             .is_null()
    }

    #[inline]
    fn pending_mask(&self) -> usize { self.pending_classes.load(Ordering::Acquire) }

    fn push(&self, class : usize, ptr : *mut u8) -> usize {
        let head = &self.heads[class];
        let mut current = head.load(Ordering::Relaxed);
        let mut misses = 0usize;
        loop {
            // SAFETY: ptr 是刚释放的 slab 对象，首字可安全写入 next 指针。
            unsafe { write_next(ptr, current) };
            match head.compare_exchange_weak(current,
                                             ptr,
                                             Ordering::Release,
                                             Ordering::Relaxed)
            {
                Ok(_) => {
                    self.pending_classes
                        .fetch_or(1usize << class, Ordering::Release);
                    return misses;
                }
                Err(observed) => {
                    misses = misses.saturating_add(1);
                    current = observed;
                }
            }
        }
    }

    fn pop(&self, class : usize) -> (Option<*mut u8>, usize) {
        let head = &self.heads[class];
        let mut current = head.load(Ordering::Acquire);
        let mut misses = 0usize;
        loop {
            if current.is_null() {
                return (None, misses);
            }
            // SAFETY: current 是队列头；只有 owner CPU pop，remote CPU 只发布新头。
            let next = unsafe { read_next(current) };
            match head.compare_exchange_weak(current,
                                             next,
                                             Ordering::AcqRel,
                                             Ordering::Acquire)
            {
                Ok(_) => return (Some(current), misses),
                Err(observed) => {
                    misses = misses.saturating_add(1);
                    current = observed;
                }
            }
        }
    }

    /// 清理 advisory pending bit，并重新检查并发 push，避免丢失维护通知。
    fn refresh_pending(&self, class : usize) {
        let bit = 1usize << class;
        if self.heads[class]
               .load(Ordering::Acquire)
               .is_null()
        {
            self.pending_classes.fetch_and(!bit, Ordering::AcqRel);
            if !self.heads[class]
                    .load(Ordering::Acquire)
                    .is_null()
            {
                self.pending_classes.fetch_or(bit, Ordering::Release);
            }
        } else {
            self.pending_classes.fetch_or(bit, Ordering::Release);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self, class : usize) -> usize {
        let mut count = 0usize;
        let mut current = self.heads[class].load(Ordering::Acquire);
        while !current.is_null() {
            count = count.saturating_add(1);
            assert!(count < 1_000_000, "remote-free queue contains a cycle");
            // SAFETY: tests call this without concurrent pop and all queued pages remain live.
            current = unsafe { read_next(current) };
        }
        count
    }
}

pub(crate) struct CpuSlab {
    caches : [SlabCache; SIZE_CLASS_COUNT],
    maintenance_cursor : usize,
}

impl CpuSlab {
    pub(crate) fn new() -> Self {
        Self { caches : core::array::from_fn(|_| SlabCache::new()),
               maintenance_cursor : 0 }
    }

    pub(crate) unsafe fn alloc(&mut self,
                               frames : &dyn HeapFrameSource,
                               class_idx : usize,
                               owner_cpu : u16,
                               remote_queues : &RemoteFreeQueues,
                               page_stats : &SlabPageStats,
                               #[cfg(feature = "slab-diagnostics")]
                               diagnostics : &SlabDiagnostics)
                               -> Option<*mut u8> {
        // 只有 remote 队列非空才执行昂贵的 atomic swap；本地 slab 分配是最热路径。
        if remote_queues.has_pending(class_idx) {
            self.drain_remote_class(frames,
                                    remote_queues,
                                    class_idx,
                                    page_stats,
                                    #[cfg(feature = "slab-diagnostics")]
                                    diagnostics);
        }
        unsafe { self.caches[class_idx].alloc(frames,
                                             class_idx,
                                             owner_cpu,
                                             page_stats,
                                             #[cfg(feature = "slab-diagnostics")]
                                             diagnostics) }
    }

    pub(crate) unsafe fn dealloc_local(&mut self,
                                       ptr : *mut u8,
                                       class_idx : usize,
                                       frames : &dyn HeapFrameSource,
                                       page_stats : &SlabPageStats,
                                       #[cfg(feature = "slab-diagnostics")]
                                       diagnostics : &SlabDiagnostics)
                                       -> bool {
        let result = unsafe { self.caches[class_idx].dealloc_local(ptr,
                                                                  class_idx,
                                                                  frames,
                                                                  page_stats,
                                                                  #[cfg(feature = "slab-diagnostics")]
                                                                  diagnostics) };
        #[cfg(feature = "slab-diagnostics")]
        if result {
            diagnostics.record_local_free(class_idx);
        }
        result
    }

    /// 把对象压入 owner CPU 对应 class 的 remote-free 队列。
    pub(crate) fn remote_push(remote_queues : &RemoteFreeQueues,
                              class : usize,
                              ptr : *mut u8)
                              -> usize {
        remote_queues.push(class, ptr)
    }

    /// 定时维护一个 pending class，使 owner 没有继续分配该 class 时仍能完成回收。
    pub(crate) fn maintain_remote(&mut self,
                                  frames : &dyn HeapFrameSource,
                                  remote_queues : &RemoteFreeQueues,
                                  page_stats : &SlabPageStats,
                                  #[cfg(feature = "slab-diagnostics")]
                                  diagnostics : &SlabDiagnostics) {
        let pending = remote_queues.pending_mask();
        if pending == 0 {
            return;
        }
        for offset in 0..SIZE_CLASS_COUNT {
            let class = (self.maintenance_cursor + offset) % SIZE_CLASS_COUNT;
            if pending & (1usize << class) == 0 {
                continue;
            }
            self.maintenance_cursor = (class + 1) % SIZE_CLASS_COUNT;
            self.drain_remote_class(frames,
                                    remote_queues,
                                    class,
                                    page_stats,
                                    #[cfg(feature = "slab-diagnostics")]
                                    diagnostics);
            return;
        }
    }

    /// 有界地把一个 class 的 remote 对象放回 owner cache。
    fn drain_remote_class(&mut self,
                          frames : &dyn HeapFrameSource,
                          remote_queues : &RemoteFreeQueues,
                          class : usize,
                          page_stats : &SlabPageStats,
                          #[cfg(feature = "slab-diagnostics")]
                          diagnostics : &SlabDiagnostics) {
        let mut drained = 0usize;
        let mut cas_misses = 0usize;
        while drained < REMOTE_DRAIN_LIMIT {
            let (object, misses) = remote_queues.pop(class);
            cas_misses = cas_misses.saturating_add(misses);
            let Some(cur) = object else {
                break;
            };
            // SAFETY: cur 来自 slab，页面仍有效。
            let hdr = unsafe { SlabPageHeader::from_obj(cur) };
            debug_assert_eq!(hdr.magic, SLAB_MAGIC);
            debug_assert_eq!(hdr.size_class(), class);
            if hdr.magic == SLAB_MAGIC && hdr.size_class() == class {
                unsafe {
                    self.caches[class].dealloc_local(cur,
                                                    class,
                                                    frames,
                                                    page_stats,
                                                    #[cfg(feature = "slab-diagnostics")]
                                                    diagnostics)
                };
                drained = drained.saturating_add(1);
            }
        }
        let hit_limit = drained == REMOTE_DRAIN_LIMIT && remote_queues.has_pending(class);
        remote_queues.refresh_pending(class);
        #[cfg(feature = "slab-diagnostics")]
        if drained != 0 || cas_misses != 0 {
            diagnostics.record_drain(drained, cas_misses, hit_limit);
        }
        #[cfg(not(feature = "slab-diagnostics"))]
        let _ = (cas_misses, hit_limit);
    }
}
