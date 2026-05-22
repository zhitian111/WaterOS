//! 任务 **实现内核**：每个任务的控制块、栈与首次运行引导，供调度器实现与 arch 入口共同使用。
//!
//! ## 与 `task-scheduler` 的边界
//!
//! - **本 crate**：[`TaskControlBlock`] 持有 [`crate::stack::KernelStack`] / 用户栈、`trap` 现场与用户态资源快照；[`TaskBootstrap`] 等把内核任务入口包装成 arch 期望的 C 可调用形态。**不负责** 全局就绪队列、时间片或“下一个任务”的选择。
//! - **`wateros-task-scheduler`**：**组装并驱动** 多个 TCB——创建任务时调用本 crate 类型完成初始化，在 `schedule`/`wait` 等路径上更新状态并触发上下文切换。
//!
//! 若仅替换调度策略，通常无需修改本 crate；若调整 TCB 字段或 trap 与栈的约定，则需与 `scheduler-impl`、平台 arch 协同更新。

#![no_std]

extern crate alloc;

mod runtime;
mod stack;
mod tcb;

pub use runtime::TaskBootstrap;
pub use tcb::TaskControlBlock;
pub use tcb::{prepare_pending_fork_user_stack_copy, prepare_pending_fork_user_stack_range,
			  take_pending_fork_user_stack_copy};
