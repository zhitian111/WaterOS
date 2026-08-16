//! 单页 slab 的页首 header 与 intrusive free list。

use core::ptr;

use super::size_class::{SizeClass, SLAB_HEADER_SIZE, SLAB_PAGE_SIZE};

pub(crate) const SLAB_MAGIC : u32 = 0x534C_4142; // "SLAB"

const _ : () = assert!(core::mem::size_of::<SlabPageHeader>() == SLAB_HEADER_SIZE);

/// 页首 header：位于页基址，反查对象归属时读取。
#[repr(C)]
pub(crate) struct SlabPageHeader {
    pub(crate) magic : u32,
    size_class : u8,
    pub(crate) owner_cpu : u16,
    total_objects : u16,
    free_objects : u16,
    first_free : *mut u8,
    pub(crate) next_partial : *mut SlabPageHeader,
    pub(crate) in_partial : bool,
    _pad : [u8; 36],
}

impl SlabPageHeader {
    /// 返回对象所在的页基址。
    pub(crate) fn page_base_of(ptr : *mut u8) -> *mut u8 {
        (ptr as usize & !(SLAB_PAGE_SIZE - 1)) as *mut u8
    }

    /// 从对象地址反查页首 header。
    ///
    /// # Safety
    /// `ptr` 必须来自 slab 且页面仍属于 slab。
    pub(crate) unsafe fn from_obj(ptr : *mut u8) -> &'static mut Self {
        unsafe { &mut *(Self::page_base_of(ptr) as *mut Self) }
    }

    /// 用一页新内存初始化 slab，并把所有对象链入 intrusive free list。
    ///
    /// # Safety
    /// `page_base` 必须页对齐、可写且已从 frame source 独占取得。
    pub(crate) unsafe fn init(page_base : *mut u8,
                              size_class : usize,
                              owner_cpu : u16)
                              -> &'static mut Self {
        let class = SizeClass::from_index(size_class);
        let obj_size = class.size();
        let object_offset = class.object_offset();
        let count = class.objects_per_slab();
        let mut first_free : *mut u8 = ptr::null_mut();
        // SAFETY: page_base 页对齐且 page 大小足够容纳 header + 对象。
        let mut cur = unsafe { page_base.add(object_offset) };
        for _ in 0..count {
            // SAFETY: cur 是本次 slab 内尚未初始化的对象地址。
            unsafe { write_next(cur, first_free) };
            first_free = cur;
            // SAFETY: 循环次数由 objects_per_slab 保证不越界。
            cur = unsafe { cur.add(obj_size) };
        }

        let hdr = page_base as *mut Self;
        unsafe { hdr.write(Self {
            magic : SLAB_MAGIC,
            size_class : size_class as u8,
            owner_cpu,
            total_objects : count as u16,
            free_objects : count as u16,
            first_free,
            next_partial : ptr::null_mut(),
            in_partial : false,
            _pad : [0; 36],
        }) };
        unsafe { &mut *hdr }
    }

    /// 弹出下一个 free 对象；没有 free 对象时返回 null。
    pub(crate) fn pop_free(&mut self) -> *mut u8 {
        let obj = self.first_free;
        if !obj.is_null() {
            // SAFETY: free list 指针均为 slab 内有效对象地址。
            self.first_free = unsafe { read_next(obj) };
            self.free_objects = self.free_objects.saturating_sub(1);
        }
        obj
    }

    /// 把一个对象压回 free list。
    pub(crate) fn push_free(&mut self, obj : *mut u8) {
        // SAFETY: 调用方保证 obj 属于本 slab 且当前不在任何 free list 中。
        unsafe { write_next(obj, self.first_free) };
        self.first_free = obj;
        self.free_objects = self.free_objects.saturating_add(1);
    }

    pub(crate) fn size_class(&self) -> usize { self.size_class as usize }

    pub(crate) fn is_full(&self) -> bool { self.free_objects == 0 }

    pub(crate) fn is_empty(&self) -> bool { self.free_objects == self.total_objects }

    /// 使页首不再能被 slab 反查识别；必须在归还 frame source 前调用。
    pub(crate) fn invalidate(&mut self) {
        self.magic = 0;
        self.next_partial = ptr::null_mut();
        self.in_partial = false;
    }
}

/// 读取 intrusive free list 下一个指针。
///
/// # Safety
/// `obj` 必须是 free list 中的对象地址。
pub(crate) unsafe fn read_next(obj : *mut u8) -> *mut u8 {
    unsafe { *(obj as *const *mut u8) }
}

/// 写入 intrusive free list 下一个指针。
///
/// # Safety
/// `obj` 必须可写且属于对应 slab。
pub(crate) unsafe fn write_next(obj : *mut u8, next : *mut u8) {
    unsafe { (obj as *mut *mut u8).write(next) };
}
