/// 架构层任务切换上下文抽象。
///
/// 该 trait 只定义“当前架构的任务切换上下文至少需要支持哪些初始化操作”，
/// 不在 API 层暴露具体寄存器布局。
pub trait ArchTaskContext: Clone + Copy + core::fmt::Debug {
    fn zero_init() -> Self;

    fn goto_entry(entry_stub: usize, kstack_top: usize) -> Self;
}
