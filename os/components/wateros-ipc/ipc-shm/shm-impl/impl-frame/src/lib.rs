#![no_std]
//! 帧后备 SysV 共享内存实现。
//!
//! `ARCH:` `registry` 管理段/附加关系，`allocation` 只管理物理帧，`global` 提供唯一锁。
//! 任何用户地址空间映射都属于 syscall/MM 层，绝不能在 SHM registry 锁内进行。

extern crate alloc;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[ipc/shm/impl-frame] self_test begin");
    assert!(api_v0::MAX_SHM_SEGMENT_SIZE > 0);
    log::info!("[ipc/shm/impl-frame] self_test complete");
}

mod allocation;
mod global;
mod registry;
mod state;

pub use api_v0::*;
pub use global::registry;
pub use registry::{ShmAttachReservation, ShmRegistry};
