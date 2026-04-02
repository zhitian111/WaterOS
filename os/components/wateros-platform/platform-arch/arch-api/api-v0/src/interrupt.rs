use crate::time::ArchTimeResult;

/// 架构层时钟中断开关原语：
/// - 只负责中断位的开/关
/// - 不负责编程下一次 timer deadline（该职责在 firmware/platform 层）
pub trait ArchTimerInterruptControl {
    /// 开启 S 态时钟中断源（例如 RISC-V 的 sie.STIE）。
    fn enable_timer_interrupt() -> ArchTimeResult<()>;

    /// 关闭 S 态时钟中断源（例如 RISC-V 的 sie.STIE）。
    fn disable_timer_interrupt() -> ArchTimeResult<()>;

    /// 开启全局中断开关（例如 RISC-V 的 sstatus.SIE）。
    fn enable_global_interrupt() -> ArchTimeResult<()>;

    /// 关闭全局中断开关（例如 RISC-V 的 sstatus.SIE）。
    fn disable_global_interrupt() -> ArchTimeResult<()>;
}

