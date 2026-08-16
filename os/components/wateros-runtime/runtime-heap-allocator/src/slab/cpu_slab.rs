//! 单个 CPU 持有的全部 size class cache。

use core::sync::atomic::{AtomicPtr, Ordering};

use super::size_class::SIZE_CLASS_COUNT;
use super::slab_cache::SlabCache;
use super::slab_page::{read_next, write_next, SlabPageHeader, SLAB_MAGIC};
use super::HeapFrameSource;
#[cfg(feature = "slab-diagnostics")]
use super::diagnostics::SlabDiagnostics;

pub(crate) struct CpuSlab {
    caches : [SlabCache; SIZE_CLASS_COUNT],
}

impl CpuSlab {
    pub(crate) fn new() -> Self {
        Self { caches : core::array::from_fn(|_| SlabCache::new()) }
    }

    pub(crate) unsafe fn alloc(&mut self,
                               frames : &dyn HeapFrameSource,
                               class_idx : usize,
                               owner_cpu : u16,
                               remote_head : &AtomicPtr<u8>,
                               #[cfg(feature = "slab-diagnostics")]
                               diagnostics : &SlabDiagnostics)
                               -> Option<*mut u8> {
        // 只有 remote 队列非空才执行昂贵的 atomic swap；本地 slab 分配是最热路径。
        if !remote_head.load(Ordering::Relaxed).is_null() {
            self.drain_remote(remote_head,
                              #[cfg(feature = "slab-diagnostics")]
                              diagnostics);
        }
        unsafe { self.caches[class_idx].alloc(frames,
                                             class_idx,
                                             owner_cpu,
                                             #[cfg(feature = "slab-diagnostics")]
                                             diagnostics) }
    }

    pub(crate) unsafe fn dealloc_local(&mut self,
                                       ptr : *mut u8,
                                       class_idx : usize,
                                       #[cfg(feature = "slab-diagnostics")]
                                       diagnostics : &SlabDiagnostics)
                                       -> bool {
        let result = unsafe { self.caches[class_idx].dealloc_local(ptr,
                                                                  class_idx,
                                                                  #[cfg(feature = "slab-diagnostics")]
                                                                  diagnostics) };
        #[cfg(feature = "slab-diagnostics")]
        if result {
            diagnostics.record_local_free(class_idx);
        }
        result
    }

    /// 把对象压入本 CPU 的 remote-free 队列；owner CPU 下一次 alloc 时 drain。
    #[cfg(not(feature = "slab-diagnostics"))]
    pub(crate) fn remote_push(remote_head : &AtomicPtr<u8>, ptr : *mut u8) {
        let mut head = remote_head.load(Ordering::Relaxed);
        loop {
            // SAFETY: ptr 是刚释放的 slab 对象，首字可安全写入 next 指针。
            unsafe { write_next(ptr, head) };
            match remote_head
                      .compare_exchange_weak(head,
                                             ptr,
                                             Ordering::AcqRel,
                                             Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(cur) => head = cur,
            }
        }
    }

    #[cfg(feature = "slab-diagnostics")]
    pub(crate) fn remote_push(remote_head : &AtomicPtr<u8>, ptr : *mut u8) -> usize {
        let mut head = remote_head.load(Ordering::Relaxed);
        let mut misses = 0usize;
        loop {
            // SAFETY: ptr 是刚释放的 slab 对象，首字可安全写入 next 指针。
            unsafe { write_next(ptr, head) };
            match remote_head
                      .compare_exchange_weak(head,
                                             ptr,
                                             Ordering::AcqRel,
                                             Ordering::Relaxed)
            {
                Ok(_) => return misses,
                Err(cur) => {
                    misses = misses.saturating_add(1);
                    head = cur;
                }
            }
        }
    }

    /// 把本 CPU remote 队列中的对象放回对应 size class 的本地 cache。
    fn drain_remote(&mut self,
                    remote_head : &AtomicPtr<u8>,
                    #[cfg(feature = "slab-diagnostics")] diagnostics : &SlabDiagnostics) {
        let head = remote_head.swap(core::ptr::null_mut(), Ordering::AcqRel);
        let mut cur = head;
        #[cfg(feature = "slab-diagnostics")]
        let mut drained = 0usize;
        while !cur.is_null() {
            // SAFETY: remote 队列只包含 slab 对象。
            let next = unsafe { read_next(cur) };
            // SAFETY: cur 来自 slab，页面仍有效。
            let hdr = unsafe { SlabPageHeader::from_obj(cur) };
            if hdr.magic == SLAB_MAGIC {
                let class = hdr.size_class();
                unsafe {
                    self.caches[class].dealloc_local(cur,
                                                    class,
                                                    #[cfg(feature = "slab-diagnostics")]
                                                    diagnostics)
                };
                #[cfg(feature = "slab-diagnostics")]
                {
                    drained = drained.saturating_add(1);
                }
            }
            cur = next;
        }
        #[cfg(feature = "slab-diagnostics")]
        diagnostics.record_drain(drained);
    }
}
