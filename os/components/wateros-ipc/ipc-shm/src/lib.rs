#![no_std]
//! SysV 共享内存聚合 crate。
//!
//! `ARCH:` 根 crate 只选择实现并重导出稳定 API。段索引、附加计数和物理帧生命周期在
//! `shm-impl/impl-frame` 中维护；syscall/MM 层负责 Linux ABI、用户 VA 预留和页表映射。

/// 稳定的 SysV SHM 类型、标志与错误。
#[cfg(feature = "api-v0")]
pub mod api {
    pub use api_v0::*;
}

/// 当前启用的帧后备共享内存实现。
#[cfg(feature = "impl-frame")]
pub use impl_frame as active_impl;

#[cfg(feature = "api-v0")]
pub use api_v0::*;

/// 保持 `ipc::shm::registry().lock()` 的既有调用 ABI；锁内仅访问 SHM 元数据。
#[cfg(feature = "impl-frame")]
pub use active_impl::{registry, ShmAttachReservation, ShmRegistry};
