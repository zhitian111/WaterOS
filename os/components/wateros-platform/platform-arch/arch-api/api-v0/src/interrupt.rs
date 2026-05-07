//! 监管态下与定时器相关的中断位及全局中断开关（ISA 层，不经 SBI）。

use crate::time::ArchTimeResult;

/// 架构层全局中断状态快照。
///
/// 该值只由当前 arch impl 解释，上层只能保存后交回 `restore_global_interrupt_state`。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchInterruptState(pub usize);

/// 架构层时钟中断开关原语：
/// - 只负责中断位的开/关
/// - 不负责编程下一次 timer deadline（该职责在 firmware/platform 层）
pub trait ArchTimerInterruptControl {
    /// 开启当前内核特权态下的时钟中断源。
    fn enable_timer_interrupt() -> ArchTimeResult<()>;

    /// 关闭当前内核特权态下的时钟中断源。
    fn disable_timer_interrupt() -> ArchTimeResult<()>;

    /// 开启当前内核特权态下的全局中断开关。
    fn enable_global_interrupt() -> ArchTimeResult<()>;

    /// 关闭当前内核特权态下的全局中断开关。
    fn disable_global_interrupt() -> ArchTimeResult<()>;

    /// 读取当前全局中断状态，用于临界区退出时恢复。
    fn read_global_interrupt_state() -> ArchTimeResult<ArchInterruptState>;

    /// 恢复先前读取到的全局中断状态。
    fn restore_global_interrupt_state(state: ArchInterruptState) -> ArchTimeResult<()>;

    /// 进入架构定义的低功耗等待，直到下一次中断或事件。
    fn wait_for_interrupt();
}
