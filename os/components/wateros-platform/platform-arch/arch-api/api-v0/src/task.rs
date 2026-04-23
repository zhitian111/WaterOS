/// 架构层任务切换上下文抽象。
///
/// 该 trait 只定义“当前架构的任务切换上下文至少需要支持哪些初始化操作”，
/// 不在 API 层暴露具体寄存器布局。
pub trait ArchTaskContext: Clone + Copy + core::fmt::Debug {
    fn zero_init() -> Self;

    fn goto_entry(entry_stub: usize, kstack_top: usize) -> Self;

    /// Build the initial context for a task that will first enter an
    /// arch-specific trampoline and then transfer control to a task runtime.
    ///
    /// The concrete arch impl decides how an opaque bootstrap pointer is
    /// encoded into the saved context, so task code does not need to write
    /// registers directly or expose bootstrap protocol details in public APIs.
    fn goto_task_entry(entry_stub: usize, kstack_top: usize, bootstrap_ptr: usize) -> Self;
}
