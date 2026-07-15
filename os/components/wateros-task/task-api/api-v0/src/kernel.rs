//内核任务
use alloc::boxed::Box;
use config::task::KERNEL_TASK_STACK_SIZE;
use core::alloc::Layout;

/// 内核任务入口：`extern "C" fn(usize) -> !`。
pub type KernelTaskEntry = extern "C" fn(usize) -> !;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TaskBootstrap {
    pub entry : KernelTaskEntry,
    pub arg : usize,
}

impl TaskBootstrap {
    /// 构造启动载荷：`entry` 为实际内核任务体，`arg` 透传给该入口。
    pub const fn new(entry : KernelTaskEntry, arg : usize) -> Self { Self { entry, arg } }

    /// 跳转到内核任务入口；仅在首次被调度到该任务时由 arch 跳板调用一次。
    #[inline]
    pub fn run(&self) -> ! { (self.entry)(self.arg) }
}


#[repr(align(16))]
struct AlignedKernelStack([u8; KERNEL_TASK_STACK_SIZE]);
/// 内核任务独占的内核栈封装。
pub struct KernelStack {
    storage : Box<AlignedKernelStack>,
    top : usize,
}
impl KernelStack {
    /// 分配内核栈；堆耗尽时返回 `None`。
    pub fn try_new() -> Option<Self> {
        let layout = Layout::new::<AlignedKernelStack>();
        let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) as *mut AlignedKernelStack };
        if ptr.is_null() {
            return None;
        }
        let storage = unsafe { Box::from_raw(ptr) };
        let stack_bottom = storage.0.as_ptr() as usize;
        let top = align_down(stack_bottom + KERNEL_TASK_STACK_SIZE,
                             16);
        Some(Self { storage, top })
    }

    /// 分配内核栈；失败时 panic（bring-up 路径）。
    pub fn new() -> Self { Self::try_new().expect("[wateros-task]kernel stack allocation failed") }

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


// `align` 必须为 2 的幂；栈顶向下对齐以满足调用约定中的 16 字节对齐要求。
#[inline]
const fn align_down(value : usize, align : usize) -> usize { value & !(align - 1) }
