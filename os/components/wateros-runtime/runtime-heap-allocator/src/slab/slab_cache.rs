//! 每个 CPU 每个 size class 的 slab cache。

use alloc::vec::Vec;

use super::slab_page::{SlabPageHeader, SLAB_MAGIC};
use super::HeapFrameSource;

pub(crate) struct SlabCache {
    current : *mut SlabPageHeader,
    partial : Vec<*mut SlabPageHeader>,
}

impl SlabCache {
    pub(crate) fn new() -> Self {
        Self { current : core::ptr::null_mut(),
               partial : Vec::new() }
    }

    /// 从当前/partial/新页中取一个对象。
    ///
    /// # Safety
    /// 调用方必须保证当前 CPU 独占访问且满足 ALLOC_SYNC。
    pub(crate) unsafe fn alloc(&mut self,
                               frames : &dyn HeapFrameSource,
                               class_idx : usize,
                               owner_cpu : u16)
                               -> Option<*mut u8> {
        loop {
            if !self.current.is_null() {
                // SAFETY: current 始终指向已初始化的 slab header。
                let hdr = unsafe { &mut *self.current };
                if !hdr.is_full() {
                    return Some(hdr.pop_free());
                }
                self.current = core::ptr::null_mut();
            }

            // 优先复用 partial 页。
            if let Some(prev) = self.partial.pop() {
                // SAFETY: partial 列表中的指针均指向有效 slab header。
                let hdr = unsafe { &mut *prev };
                if !hdr.is_empty() {
                    self.current = prev;
                    continue;
                }
                // 空页不应留在 partial；归还给 frame source。
                frames.dealloc_frame(prev as usize);
                continue;
            }

            let frame = frames.alloc_frame()?;
            // SAFETY: frame 是页对齐独占内存；init 会初始化整页。
            let hdr = unsafe { SlabPageHeader::init(frame as *mut u8, class_idx, owner_cpu) };
            self.current = hdr;
            return Some(hdr.pop_free());
        }
    }

    /// 把对象放回本 cache；对象不属于本 cache/size class 时返回 `false`。
    ///
    /// # Safety
    /// `ptr` 必须来自 slab；调用方保证 owner CPU 独占访问。
    pub(crate) unsafe fn dealloc(&mut self,
                                 ptr : *mut u8,
                                 class_idx : usize,
                                 frames : &dyn HeapFrameSource)
                                 -> bool {
        // SAFETY: ptr 由调用方保证来自 slab 页面。
        let hdr = unsafe { SlabPageHeader::from_obj(ptr) };
        if hdr.magic != SLAB_MAGIC || hdr.size_class() != class_idx {
            return false;
        }

        let is_current = hdr as *mut _ == self.current;
        hdr.push_free(ptr);

        if !is_current && hdr.is_empty() {
            self.partial.retain(|p| *p != hdr as *mut _);
            frames.dealloc_frame(hdr as *mut _ as usize);
        } else if !is_current && !self.partial.contains(&(hdr as *mut _)) {
            self.partial.push(hdr as *mut _);
        }
        true
    }
}
