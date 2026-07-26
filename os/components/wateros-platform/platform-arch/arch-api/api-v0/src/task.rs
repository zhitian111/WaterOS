//! 任务首次进入与协作式/抢占式切换所需的架构上下文构造（寄存器约定由 impl 决定）。

/// 架构层任务切换上下文抽象。
///
/// 该 trait 只定义“当前架构的任务切换上下文至少需要支持哪些初始化操作”，
/// 不在 API 层暴露具体寄存器布局。
pub trait ArchTaskContext: Clone + Copy + core::fmt::Debug {
    fn zero_init() -> Self;

    fn goto_entry(entry_stub: usize, kstack_top: usize) -> Self;

    /// 构造将先进入架构相关跳板、再转入任务运行时的初始上下文。
    ///
    /// 具体 `arch-impl` 决定如何把不透明 `bootstrap_ptr` 编码进保存的上下文，使
    /// 任务代码无需直接写寄存器或在公共 API 中暴露跳板协议细节。
    fn goto_task_entry(entry_stub: usize, kstack_top: usize, bootstrap_ptr: usize) -> Self;

    /// 返回已保存上下文的恢复 PC，用于调度器诊断上下文损坏。
    fn return_address(&self) -> usize;

    /// 返回已保存上下文的栈指针，用于调度器诊断上下文损坏。
    fn stack_pointer(&self) -> usize;
}
