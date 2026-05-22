//! 内核任务与用户任务的 **独占栈** 分配：固定大小、16 字节对齐，供
//! `TaskControlBlock` 与 arch 入口约定栈顶。
//!
//! 尺寸为 bring-up 常量；若与链接脚本或 guard 页策略冲突，应在此文件与 MM
//! 文档中一并调整。

use alloc::boxed::Box;
use core::alloc::Layout;

const KERNEL_TASK_STACK_SIZE : usize = 32 * 1024;
const USER_TASK_STACK_SIZE : usize = 16 * 1024;

#[repr(align(16))]
struct AlignedKernelStack([u8; KERNEL_TASK_STACK_SIZE]);

#[repr(align(16))]
struct AlignedUserStack([u8; USER_TASK_STACK_SIZE]);

/// 内核任务独占的内核栈封装。
pub struct KernelStack {
    storage : Box<AlignedKernelStack>,
    top : usize,
}

impl KernelStack {
    pub(crate) fn new() -> Self {
        // 使用 alloc_zeroed 避免 [0; 32KB] 在任务内核栈上产生临时量
        let layout = Layout::new::<AlignedKernelStack>();
        let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) as *mut AlignedKernelStack };
        let storage = unsafe { Box::from_raw(ptr) };
        let stack_bottom = storage.0.as_ptr() as usize;
        let top = align_down(stack_bottom + KERNEL_TASK_STACK_SIZE,
                             16);
        Self { storage, top }
    }

    #[inline]
    /// 返回当前内核栈的栈顶地址。
    pub fn top(&self) -> usize {
        debug_assert_eq!(align_down(self.storage
                                        .0
                                        .as_ptr() as usize +
                                    KERNEL_TASK_STACK_SIZE,
                                    16),
                         self.top);
        self.top
    }
}

/// 用户任务独占的用户栈封装。
pub struct UserStack {
    storage : Box<AlignedUserStack>,
    bottom : usize,
    top : usize,
}

impl UserStack {
    pub(crate) fn new() -> Self {
        // 使用 alloc_zeroed 避免 [0; 16KB] 在栈上产生临时量
        let layout = Layout::new::<AlignedUserStack>();
        let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) as *mut AlignedUserStack };
        let storage = unsafe { Box::from_raw(ptr) };
        let stack_bottom = storage.0.as_ptr() as usize;
        let top = align_down(stack_bottom + USER_TASK_STACK_SIZE, 16);
        Self { storage,
               bottom : stack_bottom,
               top }
    }

    #[inline]
    /// 返回当前用户栈的栈底地址。
    pub fn bottom(&self) -> usize {
        debug_assert_eq!(self.storage
                             .0
                             .as_ptr() as usize,
                         self.bottom);
        self.bottom
    }

    #[inline]
    /// 返回当前用户栈的栈顶地址。
    pub fn top(&self) -> usize {
        debug_assert_eq!(align_down(self.storage
                                        .0
                                        .as_ptr() as usize +
                                    USER_TASK_STACK_SIZE,
                                    16),
                         self.top);
        self.top
    }

    #[inline]
    /// 返回当前用户栈大小。
    pub const fn size(&self) -> usize { USER_TASK_STACK_SIZE }
}

// `align` 必须为 2 的幂；栈顶向下对齐以满足调用约定中的 16 字节对齐要求。
#[inline]
const fn align_down(value : usize, align : usize) -> usize { value & !(align - 1) }
