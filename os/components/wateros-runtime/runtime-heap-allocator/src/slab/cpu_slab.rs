//! 单个 CPU 持有的全部 size class cache。

use super::size_class::SIZE_CLASS_COUNT;
use super::slab_cache::SlabCache;
use super::HeapFrameSource;

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
                               owner_cpu : u16)
                               -> Option<*mut u8> {
        unsafe { self.caches[class_idx].alloc(frames, class_idx, owner_cpu) }
    }

    pub(crate) unsafe fn dealloc(&mut self,
                                 ptr : *mut u8,
                                 class_idx : usize)
                                 -> bool {
        unsafe { self.caches[class_idx].dealloc(ptr, class_idx) }
    }
}
