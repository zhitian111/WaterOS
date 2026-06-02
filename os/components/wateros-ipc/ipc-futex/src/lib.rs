#![no_std]
//! Futex IPC 聚合：导出版本化 API 契约与当前启用的实现类型。
//!
//! - [`api`]：队列键、错误、[`KernelFutexOps`] trait 与 robust 布局（`futex-api/api-v0`）。
//! - [`active_impl`]：当前 feature 选中的实现命名空间。
//! - 根层再导出调用方常用的具体类型名（[`FutexHub`] 等）。

/// 版本化 futex API 契约。
pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-task")]
/// 当前 futex 实现命名空间。
pub use impl_task as active_impl;

#[cfg(all(feature = "impl-dummy", not(feature = "impl-task")))]
/// 当前 futex 实现命名空间（dummy）。
pub use impl_dummy as active_impl;

pub use api_v0::{
    FutexError, FutexKey, FutexResult, FutexWaitOutcome, KernelFutexOps, RobustListHead,
    FUTEX_OWNER_DIED, FUTEX_PRIVATE_FLAG, FUTEX_TID_MASK, ROBUST_LIST_ENTRY_SIZE,
    ROBUST_LIST_HEAD_SIZE, ROBUST_LIST_LIMIT,
};

#[cfg(any(feature = "impl-task", feature = "impl-dummy"))]
pub use active_impl::FutexHub;

/// 聚合层自检：串联 API 与当前激活 impl。
#[cfg(feature = "impl-task")]
pub fn test() {
    api_v0::test();
    active_impl::test();
}

#[cfg(all(feature = "impl-dummy", not(feature = "impl-task")))]
pub fn test() {
    api_v0::test();
    active_impl::test();
}
