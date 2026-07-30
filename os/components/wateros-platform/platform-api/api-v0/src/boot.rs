//! 引导参数与上下文：由 `platform-impl` 将固件传入的原始寄存器约定映射为类型化值。

use core::{fmt::Debug, option::Option};

/// 固件/引导加载器传入内核的**最小**参数面（`a0`/`a1`/… 等约定由实现解释）。
///
/// 默认方法返回 `None`：具体板级或 QEMU profile 应覆盖需要暴露的参数槽位。
pub trait PlatformBootArgs: Debug + Clone + Copy {
    /// 首参（如 RISC-V OpenSBI 的 hart id）；未使用则 `None`。
    #[inline]
    fn arg0(&self) -> Option<usize> {
        None
    }
    /// 次参（如 DTB 物理指针）；未使用则 `None`。
    #[inline]
    fn arg1(&self) -> Option<usize> {
        None
    }
    /// 第三参（板级扩展）；未使用则 `None`。
    #[inline]
    fn arg2(&self) -> Option<usize> {
        None
    }
    // 更多槽位由板级实现按需覆盖。
}

// 入口汇编 `_start` 与具体实现约定一致，由链接脚本与启动代码提供，不在本 trait 中表达。
