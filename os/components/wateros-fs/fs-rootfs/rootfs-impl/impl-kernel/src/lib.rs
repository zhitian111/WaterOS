#![no_std]

//! Rootfs 聚合门面：按语义组合状态注册与卷挂载操作。

#![allow(clippy::all)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use api_v0::RootFsManager;
use fs_api_v0::FsImpl;
use spin::Mutex;

#[path = "state.rs"]
mod state;
pub use state::{bump_mount_generation, mount_generation};
#[path = "registry.rs"]
mod registry;
pub use registry::*;
#[path = "mount.rs"]
mod mount;
pub use mount::*;
