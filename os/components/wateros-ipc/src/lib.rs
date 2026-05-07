#![no_std]
//! WaterOS IPC 聚合 crate：导出版本化 `api` 门面、与任务系统对齐的 `waitqueue`，并在 feature 下挂载具体 `active_impl`。
//!
//! 当前默认仅包含 dummy 实现与等待队列包装；管道、共享内存等子目录 crate 尚未接入本聚合包的依赖图。

pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-dummy")]
/// 编译期选中的 IPC 实现命名空间；dummy 阶段用于占位链接与后续替换。
pub use impl_dummy as active_impl;

pub mod waitqueue {
    pub use ::waitqueue::*;
}
