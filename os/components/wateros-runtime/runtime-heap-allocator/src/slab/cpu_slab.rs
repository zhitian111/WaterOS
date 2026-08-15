//! 单个 CPU 持有的全部 size class cache。

use super::size_class::SIZE_CLASS_COUNT;
use super::slab_cache::SlabCache;
use super::slab_page::{read_next, write_next, SlabPageHeader, SLAB_MAGIC};
use super::HeapFrameSource;

pub(crate) struct CpuSlab {
    caches : [SlabCache; SIZE_CLASS_COUNT],
    remote_head : spin::Mutex<*mut u8>,
}

impl CpuSlab {
    pub(crate) fn new() -> Self {
        Self { caches : core::array::from_fn(|_| SlabCache::new()),
               remote_head : spin::Mutex::new(core::ptr::null_mut()) }
    }

    pub(crate) unsafe fn alloc(&mut self,
                               frames : &dyn HeapFrameSource,
                               class_idx : usize,
                               owner_cpu : u16)
                               -> Option<*mut u8> {
        self.drain_remote();
        unsafe { self.caches[class_idx].alloc(frames, class_idx, owner_cpu) }
    }

    pub(crate) unsafe fn dealloc_local(&mut self,
                                       ptr : *mut u8,
                                       class_idx : usize)
                                       -> bool {
        unsafe { self.caches[class_idx].dealloc_local(ptr, class_idx) }
    }

    /// 把对象压入本 CPU 的 remote-free 队列；owner CPU 下一次 alloc 时 drain。
    pub(crate) fn remote_push(&self, ptr : *mut u8) {
        let mut head = self.remote_head.lock();
        // SAFETY: ptr 是刚释放的 slab 对象，首字可安全写入 next 指针。
        unsafe { write_next(ptr, *head) };
        *head = ptr;
    }

    /// 把本 CPU remote 队列中的对象放回对应 size class 的本地 cache。
    fn drain_remote(&mut self) {
        let head = {
            let mut guard = self.remote_head.lock();
            let head = *guard;
            *guard = core::ptr::null_mut();
            head
        };
        let mut cur = head;
        while !cur.is_null() {
            // SAFETY: remote 队列只包含 slab 对象。
            let next = unsafe { read_next(cur) };
            // SAFETY: cur 来自 slab，页面仍有效。
            let hdr = unsafe { SlabPageHeader::from_obj(cur) };
            if hdr.magic == SLAB_MAGIC {
                let class = hdr.size_class();
                unsafe { self.caches[class].dealloc_local(cur, class) };
            }
            cur = next;
        }
    }
}
