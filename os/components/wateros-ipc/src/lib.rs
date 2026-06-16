#![no_std]
//! WaterOS IPC 聚合 crate：导出版本化 `api` 门面、与任务系统对齐的 `waitqueue`，并在 feature 下挂载 pipe 等 IPC 对象。
//!
//! 当前默认包含 dummy 总实现、等待队列包装与可选 pipe/shm/futex/signal 等 IPC 对象。
//!
//! 与上下层边界：本 crate 不负责具体 syscall 号或 ABI；`api` 与 `active_impl` 由独立子包演进，聚合层只做重导出与 feature 选路。

/// 版本化 IPC 协议门面（当前为 v0）；真实系统调用号、句柄与错误枚举在对应 `api-v0` crate 中演进。
pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-dummy")]
/// 编译期选中的 IPC 实现命名空间；dummy 阶段用于占位链接与后续替换。
pub use impl_dummy as active_impl;

/// 任务等待队列在 IPC 命名空间下的视图；语义委托 `wateros_task`，便于 IPC 子系统依赖单一 crate 边界。
pub mod waitqueue {
    pub use ::waitqueue::*;
}

#[cfg(feature = "pipe")]
/// 管道 IPC 对象与错误契约。
pub mod pipe {
    pub use ::pipe::*;
}

#[cfg(feature = "futex")]
/// Futex 等待/唤醒与 robust 链表契约。
pub mod futex {
    pub use ::futex::*;
}

#[cfg(feature = "shm")]
/// SysV shared memory registry and physical page lifecycle.
pub mod shm {
    pub use ::shm::*;
}

#[cfg(feature = "signal")]
/// Signal action, mask and pending-set state.
pub mod signal {
    pub use ::signal::*;
}
