//! 每个 CPU 每个 size class 的 slab cache。

#[cfg(feature = "slab-diagnostics")]
use super::diagnostics::SlabDiagnostics;
use super::page_stats::SlabPageStats;
use super::slab_page::{SlabPageHeader, SLAB_MAGIC};
use super::HeapFrameSource;

pub(crate) struct SlabCache {
    current : *mut SlabPageHeader,
    partial_head : *mut SlabPageHeader,
    warm_empty : *mut SlabPageHeader,
}

impl SlabCache {
    pub(crate) fn new() -> Self {
        Self { current : core::ptr::null_mut(),
               partial_head : core::ptr::null_mut(),
               warm_empty : core::ptr::null_mut() }
    }

    /// 从当前/partial/新页中取一个对象。
    ///
    /// # Safety
    /// 调用方必须保证当前 CPU 独占访问且满足 ALLOC_SYNC。
    pub(crate) unsafe fn alloc(&mut self,
                               frames : &dyn HeapFrameSource,
                               class_idx : usize,
                               owner_cpu : u16,
                               page_stats : &SlabPageStats,
                               #[cfg(feature = "slab-diagnostics")]
                               diagnostics : &SlabDiagnostics)
                               -> Option<*mut u8> {
        loop {
            if !self.current.is_null() {
                // SAFETY: current 始终指向已初始化的 slab header。
                let hdr = unsafe { &mut *self.current };
                if !hdr.is_full() {
                    let was_empty = hdr.is_empty();
                    let obj = hdr.pop_free();
                    if was_empty {
                        #[cfg(feature = "slab-diagnostics")]
                        diagnostics.record_page_became_nonempty(class_idx);
                    }
                    #[cfg(feature = "slab-diagnostics")]
                    diagnostics.record_local_hit(class_idx);
                    return Some(obj);
                }
                self.current = core::ptr::null_mut();
            }

            // 优先复用 partial 页。
            if !self.partial_head.is_null() {
                let prev = self.partial_head;
                // SAFETY: partial 列表中的指针均指向有效 slab header。
                let hdr = unsafe { &mut *prev };
                self.partial_head = hdr.next_partial;
                hdr.next_partial = core::ptr::null_mut();
                hdr.in_partial = false;
                debug_assert!(!hdr.is_empty());
                self.current = prev;
                continue;
            }

            if !self.warm_empty.is_null() {
                page_stats.record_became_nonempty(class_idx);
                self.current = self.warm_empty;
                self.warm_empty = core::ptr::null_mut();
                continue;
            }

            #[cfg(not(feature = "slab-diagnostics"))]
            let frame = frames.alloc_frame()?;
            #[cfg(feature = "slab-diagnostics")]
            let Some(frame) = frames.alloc_frame() else {
                diagnostics.record_oom();
                return None;
            };
            #[cfg(feature = "slab-diagnostics")]
            diagnostics.record_frame_refill(class_idx);
            page_stats.record_refill(class_idx);
            // SAFETY: frame 是页对齐独占内存；init 会初始化整页。
            let hdr = unsafe { SlabPageHeader::init(frame as *mut u8, class_idx, owner_cpu) };
            self.current = hdr;
        }
    }

    /// 把对象放回本 cache；对象不属于本 cache/size class 时返回 `false`。
    ///
    /// # Safety
    /// `ptr` 必须来自 slab；调用方保证 owner CPU 独占访问。
    pub(crate) unsafe fn dealloc_local(&mut self,
                                       ptr : *mut u8,
                                       class_idx : usize,
                                       frames : &dyn HeapFrameSource,
                                       page_stats : &SlabPageStats,
                                       #[cfg(feature = "slab-diagnostics")]
                                       diagnostics : &SlabDiagnostics)
                                       -> bool {
        // SAFETY: ptr 由调用方保证来自 slab 页面。
        let hdr = unsafe { SlabPageHeader::from_obj(ptr) };
        if hdr.magic != SLAB_MAGIC || hdr.size_class() != class_idx {
            return false;
        }

        let is_current = hdr as *mut _ == self.current;
        hdr.push_free(ptr);
        if !hdr.is_empty() {
            if !is_current && !hdr.in_partial {
                hdr.in_partial = true;
                hdr.next_partial = self.partial_head;
                self.partial_head = hdr;
            }
            return true;
        }

        #[cfg(feature = "slab-diagnostics")]
        diagnostics.record_page_became_empty(class_idx);

        if is_current {
            // current 与 detached reserve 最多各保留一页，形成两页迟滞以减少阶段抖动。
            return true;
        }

        if hdr.in_partial {
            self.remove_partial(hdr);
        }

        if self.current.is_null() {
            self.current = hdr;
        } else if self.warm_empty.is_null() {
            self.warm_empty = hdr;
            page_stats.record_became_empty(class_idx);
        } else {
            unsafe {
                self.reclaim(hdr,
                             frames,
                             page_stats,
                             class_idx,
                             #[cfg(feature = "slab-diagnostics")]
                             diagnostics);
            }
        }
        true
    }

    fn remove_partial(&mut self, target : *mut SlabPageHeader) {
        let mut link = &mut self.partial_head as *mut *mut SlabPageHeader;
        while unsafe { !(*link).is_null() } {
            let candidate = unsafe { *link };
            if candidate == target {
                let next = unsafe { (*candidate).next_partial };
                unsafe {
                    *link = next;
                    (*candidate).next_partial = core::ptr::null_mut();
                    (*candidate).in_partial = false;
                }
                return;
            }
            link = unsafe { &mut (*candidate).next_partial };
        }
        debug_assert!(false, "empty slab page missing from partial list");
    }

    /// 回收前清除所有 cache 引用并使 header 失效。
    ///
    /// # Safety
    /// `page` 必须属于本 cache 且已经完全空闲。
    unsafe fn reclaim(&mut self,
                      page : *mut SlabPageHeader,
                      frames : &dyn HeapFrameSource,
                      page_stats : &SlabPageStats,
                      class_idx : usize,
                      #[cfg(feature = "slab-diagnostics")] diagnostics : &SlabDiagnostics) {
        debug_assert!(page != self.current);
        debug_assert!(page != self.warm_empty);
        debug_assert!(unsafe { (*page).is_empty() });
        unsafe { (*page).invalidate() };
        page_stats.record_reclaim(class_idx);
        #[cfg(feature = "slab-diagnostics")]
        diagnostics.record_frame_reclaim(class_idx);
        frames.dealloc_frame(page as usize);
    }
}
