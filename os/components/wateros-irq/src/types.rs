//! IRQ 核心基础类型：虚拟中断号、硬件中断号、触发方式、亲和性与错误类型。

/// 内核统一的虚拟中断号（由 irq domain 分配；驱动与子系统只认识它）。
///
/// 当前实现中 virq 即中断线注册表下标，稳定且单调递增；上层不得假设具体取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Virq(pub u32);

/// 中断控制器内部的中断号（hwirq），只在 irqchip / irq domain 层可见。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HwIrq(pub u32);

/// 中断触发方式（来自 DTB 或平台默认；缺省为 [`IrqTrigger::Unknown`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqTrigger {
    /// 高电平触发。
    LevelHigh,
    /// 低电平触发。
    LevelLow,
    /// 上升沿触发。
    EdgeRising,
    /// 下降沿触发。
    EdgeFalling,
    /// 未知 / 未解析。
    Unknown,
}

/// 中断亲和性：允许任意 CPU，或固定到某 CPU 集合（位图，bit i 表示 CPU i）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqAffinity {
    /// 任意在线 CPU 均可接收。
    Any,
    /// 仅允许位图中置位的 CPU 接收。
    Cpus(u64),
}

/// IRQ 子系统错误分类（不映射到用户 errno）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqError {
    /// virq / hwirq 非法或越界。
    Invalid,
    /// 当前控制器 / 平台不支持该操作。
    Unsupported,
    /// 资源已被占用或注册冲突。
    Busy,
    /// 未找到对应注册项。
    NotFound,
    /// 控制器底层操作失败。
    Controller,
}

/// [`IrqError`] 上的结果别名。
pub type IrqResult<T> = core::result::Result<T, IrqError>;
