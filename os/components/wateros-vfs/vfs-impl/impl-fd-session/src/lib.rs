//! per-task fd 表与控制台/pipe [`VfsIoHandle`] 实现。

#![no_std]

extern crate alloc;

pub mod char_dev_handle;
pub mod cwd;
pub mod file_lock;
pub mod handles;
pub mod interrupt_guard;
pub mod registry;

pub use char_dev_handle::{CharDevHandle, is_rtc_dev_path, metadata_for_devfs_path};

pub use cwd::{PerTaskCwdRegistry, PATH_MAX};
pub use file_lock::{Flock, InodeKey, LOCK_EX, LOCK_NB, LOCK_SH, LOCK_UN};
pub use handles::{
    ConsoleInHandle, ConsoleOutHandle, CpuDmaLatencyDeviceHandle, NullDeviceHandle,
    PipeReadHandle, PipeWriteHandle, UnixStreamPairEnd, UrandomDeviceHandle, ZeroDeviceHandle,
    pipe_handle_pair, poll_pipe_smoke, stream_pair_handle_pair, stream_pair_smoke,
};
pub use registry::PerTaskFdRegistry;
pub use interrupt_guard::with_interrupt_disabled;
