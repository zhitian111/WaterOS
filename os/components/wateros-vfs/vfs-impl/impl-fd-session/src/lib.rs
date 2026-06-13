//! per-task fd 表与控制台/pipe [`VfsIoHandle`] 实现。

#![no_std]

extern crate alloc;

pub mod char_dev_handle;
pub mod cwd;
pub mod handles;
pub mod registry;

pub use char_dev_handle::{CharDevHandle, is_rtc_dev_path};

pub use cwd::{PerTaskCwdRegistry, PATH_MAX};
pub use handles::{
    ConsoleInHandle, ConsoleOutHandle, NullDeviceHandle, PipeReadHandle, PipeWriteHandle,
    UrandomDeviceHandle, ZeroDeviceHandle, pipe_handle_pair, poll_pipe_smoke,
};
pub use registry::PerTaskFdRegistry;
