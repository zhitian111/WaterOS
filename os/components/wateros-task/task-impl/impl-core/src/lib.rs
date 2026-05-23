//! 任务 **实现内核**：每个任务的控制块与任务类型专属资源，供调度器实现与 arch
//! 入口共同使用。
//!
//! - **本 crate**：[`TaskControlBlock`] 持有任务通用元数据与 [`TaskInner`]
//!   （区分 Idle / Kernel / User），内核栈与用户栈由对应的任务类型管理。
//! - **`wateros-task-scheduler`**：**组装并驱动** 多个 TCB——创建任务时调用本
//!   crate 类型完成初始化，在 `schedule`/`wait`
//!   等路径上更新状态并触发上下文切换。

#![no_std]

extern crate alloc;

mod tcb;

pub use api_v0::TaskBootstrap;
pub use tcb::TaskControlBlock;
