//! 任务子系统 **公共 API 版本
//! v0**：类型与轻量构造，供调度器、实现层与内核其他模块共享。
//!
//! 本 crate **不** 依赖具体调度算法或 TCB
//! 布局；新增字段或状态机时应在此明确语义契约，再同步 `impl-core` 与
//! `scheduler-impl`。

#![no_std]
extern crate alloc;
mod kernel;
mod process;
mod sched;
mod snapshot;
mod task;
mod user;
mod wait;
pub use kernel::{KernelStack, KernelTaskEntry, TaskBootstrap};
pub use process::{
    AddressSpaceRef, CloneFlags, ProcessDescriptor, ProcessId, ProcessState, ProcessTaskDescriptor,
    ProcessTaskRole, ProcessTaskState, ResourceLimit, SetResourceLimitError, TaskClearTid,
    ThreadId,
};
pub use sched::{
    SchedError, SchedParam, SchedPolicy, SchedulableCheck, SCHED_CPU_MASK_MIN_BYTES,
    SCHED_CPU_MASK_RET_BYTES,
};
pub use snapshot::{TaskSnapshot, TaskTrapSnapshot};
pub use task::{
    ExitedTask, TaskBlockReason, TaskExitCode, TaskId, TaskKind, TaskRuntimeStats, TaskState,
    TaskTick, WaitQueueId, IDLE_TASK_ID,
};
pub use user::{AddressSpaceHandle, UserImageInfo, UserStack, UserTask, UserTaskEntryPc};
pub use wait::{TaskWaitHandle, TaskWaitResult, TaskWaitTarget};
