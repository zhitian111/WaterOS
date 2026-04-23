use alloc::boxed::Box;

const KERNEL_TASK_STACK_SIZE: usize = 32 * 1024;
const USER_TASK_STACK_SIZE: usize = 16 * 1024;

#[repr(align(16))]
struct AlignedKernelStack([u8; KERNEL_TASK_STACK_SIZE]);

#[repr(align(16))]
struct AlignedUserStack([u8; USER_TASK_STACK_SIZE]);

/// 内核任务独占的内核栈封装。
pub struct KernelStack {
    storage: Box<AlignedKernelStack>,
    top: usize,
}

impl KernelStack {
    pub(crate) fn new() -> Self {
        let storage = Box::new(AlignedKernelStack([0; KERNEL_TASK_STACK_SIZE]));
        let stack_bottom = storage.0.as_ptr() as usize;
        let top = align_down(stack_bottom + KERNEL_TASK_STACK_SIZE, 16);
        Self { storage, top }
    }

    #[inline]
    /// 返回当前内核栈的栈顶地址。
    pub fn top(&self) -> usize {
        debug_assert_eq!(
            align_down(self.storage.0.as_ptr() as usize + KERNEL_TASK_STACK_SIZE, 16),
            self.top
        );
        self.top
    }
}

/// 用户任务独占的用户栈封装。
pub struct UserStack {
    storage: Box<AlignedUserStack>,
    bottom: usize,
    top: usize,
}

impl UserStack {
    pub(crate) fn new() -> Self {
        let storage = Box::new(AlignedUserStack([0; USER_TASK_STACK_SIZE]));
        let stack_bottom = storage.0.as_ptr() as usize;
        let top = align_down(stack_bottom + USER_TASK_STACK_SIZE, 16);
        Self {
            storage,
            bottom: stack_bottom,
            top,
        }
    }

    #[inline]
    /// 返回当前用户栈的栈底地址。
    pub fn bottom(&self) -> usize {
        debug_assert_eq!(self.storage.0.as_ptr() as usize, self.bottom);
        self.bottom
    }

    #[inline]
    /// 返回当前用户栈的栈顶地址。
    pub fn top(&self) -> usize {
        debug_assert_eq!(
            align_down(self.storage.0.as_ptr() as usize + USER_TASK_STACK_SIZE, 16),
            self.top
        );
        self.top
    }

    #[inline]
    /// 返回当前用户栈大小。
    pub const fn size(&self) -> usize { USER_TASK_STACK_SIZE }
}

#[inline]
const fn align_down(value: usize, align: usize) -> usize { value & !(align - 1) }
