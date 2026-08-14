//! Devfs 聚合门面：设备节点状态、设备查找与 FsImpl 注册分别按语义组织。

extern crate alloc;

use alloc::{format, string::String, string::ToString, vec::Vec};
use api_v0::{DevFsManager, DevNode};
use driver_block_api_v0::{block_device_at, block_device_count, SharedBlockDevice};
use driver_character_api_v0::{character_device_at, character_device_count,
                               character_device_kind_at, CharacterDeviceKind,
                               SharedCharacterDevice};
use fs_api_v0::{FsAccessMode, FsCapability, FsError, FsImpl, FsKind, FsResult, SharedFs};
use spin::Mutex;

#[path = "manager.rs"]
mod manager;
pub use manager::*;
#[path = "fs_impl.rs"]
mod fs_impl;
pub use fs_impl::*;
