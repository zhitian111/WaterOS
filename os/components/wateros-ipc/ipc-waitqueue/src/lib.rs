#![no_std]
//! IPC waitqueue 聚合 crate：导出版本化 API 契约，并按 feature 挂载当前实现。

#[cfg(feature = "api-v0")]
pub mod api {
    pub use api_v0::*;
}

#[cfg(feature = "impl-task")]
pub use impl_task as active_impl;

#[cfg(feature = "api-v0")]
pub use api_v0::{IpcWaitQueueOps, TaskId, TaskTick, TaskWaitHandle, TaskWaitResult, WaitQueueId};

#[cfg(feature = "impl-task")]
pub use active_impl::WaitQueue;
