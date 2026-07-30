#![no_std]

//! **架构 API v0**：ISA 相关的类型与 trait（trap、时间、任务上下文、中断控制等）。
//!
//! 本 crate **不**依赖任何固件、SBI 或板级 profile；与 `wateros-platform-api-v0` 正交，
//! 由上层 `wateros-platform` 或 `arch-impl` 在必要时组合调用。

/// 当前 CPU 标识与本核早期初始化结果类型。
pub mod cpu;
/// 定时器与全局中断在 ISA 层的开关原语。
pub mod interrupt;
/// 组合层 trap 路由：单入口 + 运行期注册，避免 `arch-impl` 直接依赖 `task`/`syscall`。
pub mod kernel_trap;
pub mod paging;
/// 任务初次运行与切换所需的架构上下文构造。
pub mod task;
/// 单调时间计数与可选频率查询（**不含** `set_timer` / SBI）。
pub mod time;
/// 异常与中断：trap 帧、原因解码、与 syscall ABI 的读写接口。
pub mod trap;
