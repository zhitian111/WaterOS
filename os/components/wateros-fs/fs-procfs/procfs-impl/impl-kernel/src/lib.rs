#![no_std]

//! 本模块代码由AI完成

//! 内核 procfs：从 task/cred/mm 与 VFS 回调生成 `/proc` 内容。

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use api_v0::*;
use core::fmt::Write;
use fs_api_v0::{FsAccessMode, FsCapability, FsImpl, FsKind};
use network::{SocketKind, SocketState};
use spin::Mutex;
use task::{ProcessId, ProcessState, TaskState, TaskWaitTarget, ThreadId};

#[path = "callbacks.rs"]
mod callbacks;
pub use callbacks::*;
pub(crate) use callbacks::{argv_for, cwd_for, env_for, exe_for, fds_for, fd_target_for, mount_lines, root_for, sysvipc_table,
                            thread_comm_str, timer_slack_for};
#[path = "path.rs"]
mod path;
pub(crate) use path::*;
#[path = "render.rs"]
mod render;
pub(crate) use render::*;
#[path = "view.rs"]
mod view;
pub use view::{view, KernelProcFs};
#[path = "fs_impl.rs"]
mod fs_impl;
pub use fs_impl::{test, IMPL, KernelProcFsImpl};
#[cfg(feature = "self_test")]
pub use fs_impl::self_test;
