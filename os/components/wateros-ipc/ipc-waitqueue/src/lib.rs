#![no_std]
//! IPC waitqueue 聚合 crate：导出版本化 API 契约，并按 feature 挂载当前实现。
//!
//! 默认 `impl-task`：薄包装 `wateros_task::WaitQueue`，供 futex/pipe 等 IPC 对象阻塞与唤醒。

#[cfg(feature = "api-v0")]
pub mod api {
    pub use api_v0::*;
}

#[cfg(feature = "impl-task")]
pub use impl_task as active_impl;

#[cfg(feature = "api-v0")]
pub use api_v0::{IpcWaitQueueOps, TaskId, TaskTick, TaskWaitResult, WaitQueueId};

#[cfg(feature = "impl-task")]
pub use active_impl::WaitQueue;
