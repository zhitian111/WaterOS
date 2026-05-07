//! 任务子系统 **公共 API 版本 v0**：类型与轻量构造，供调度器、实现层与内核其他模块共享。
//!
//! 本 crate **不** 依赖具体调度算法或 TCB 布局；新增字段或状态机时应在此明确语义契约，再同步 `impl-core` 与 `scheduler-impl`。

#![no_std]

mod snapshot;
mod task;
mod user;
mod wait;

pub use snapshot::{TaskSnapshot, TaskTrapSnapshot};
pub use task::{
    ExitedTask, KernelTaskEntry, TaskBlockReason, TaskExitCode, TaskId, TaskKind, TaskRuntimeStats,
    TaskState, TaskTick, UserTaskEntryPc, WaitQueueId, IDLE_TASK_ID,
};
pub use user::{AddressSpaceHandle, UserImageInfo, UserTaskResources, UserTaskSpec};
pub use wait::{TaskWaitHandle, TaskWaitResult, TaskWaitTarget};
