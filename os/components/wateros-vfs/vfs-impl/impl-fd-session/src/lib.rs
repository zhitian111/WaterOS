//! per-task fd 表与控制台/pipe [`VfsIoHandle`] 实现。
//! 本模块代码由AI完成

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
    NamedPipeHandle, PipeReadHandle, PipeWriteHandle, UnixStreamPairEnd, UrandomDeviceHandle,
    ZeroDeviceHandle, open_named_pipe, pipe_handle_pair, pipe_handle_pair_with_flags,
    poll_pipe_smoke, stream_pair_handle_pair, stream_pair_smoke,
};
pub use registry::PerTaskFdRegistry;
pub use interrupt_guard::with_interrupt_disabled;

pub fn test() {
    ipc::pipe::test();
    driver_character_api_v0::test();
    char_dev_handle::read_lease_self_test();
    handles::read_lease_self_test();
}
