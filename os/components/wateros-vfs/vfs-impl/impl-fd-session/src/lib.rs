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
#[cfg(feature = "user-graphics")]
mod user_graphics;

pub use char_dev_handle::{CharDevHandle, is_rtc_dev_path, metadata_for_devfs_path};

pub use cwd::{PerTaskCwdRegistry, PATH_MAX};
pub use file_lock::{Flock, InodeKey, LOCK_EX, LOCK_NB, LOCK_SH, LOCK_UN};
pub use handles::{
    ConsoleInHandle, ConsoleOutHandle, CpuDmaLatencyDeviceHandle, NullDeviceHandle,
    NamedPipeHandle, PipeReadHandle, PipeWriteHandle, UnixStreamPairEnd, UrandomDeviceHandle,
    ZeroDeviceHandle, open_named_pipe, pipe_handle_pair, pipe_handle_pair_with_flags,
    poll_pipe_smoke, stream_pair_handle_pair, stream_pair_smoke,
};
pub use registry::{PerTaskFdRegistry, poll_console_input_once};
pub use interrupt_guard::with_interrupt_disabled;

#[cfg(feature = "user-graphics")]
pub use user_graphics::{
    initialize_user_graphics_devices, open_special_device, special_device_exists,
    special_device_metadata, special_device_paths, user_graphics_input_worker,
};

#[cfg(not(feature = "user-graphics"))]
pub fn initialize_user_graphics_devices() -> bool { false }

#[cfg(not(feature = "user-graphics"))]
pub extern "C" fn user_graphics_input_worker(_arg : usize) -> ! {
    loop { task::sleep_for_ticks(1); }
}

#[cfg(not(feature = "user-graphics"))]
pub fn special_device_exists(_path : &str) -> bool { false }

#[cfg(not(feature = "user-graphics"))]
pub fn special_device_metadata(_path : &str) -> Option<api_v0::VfsMetadata> { None }

#[cfg(not(feature = "user-graphics"))]
pub fn special_device_paths() -> alloc::vec::Vec<alloc::string::String> { alloc::vec::Vec::new() }

#[cfg(not(feature = "user-graphics"))]
pub fn open_special_device(
    _path : &str,
    _accmode : u32,
    _nonblocking : bool,
) -> Option<api_v0::VfsResult<alloc::boxed::Box<dyn api_v0::VfsIoHandle>>> { None }

pub fn test() {
    ipc::pipe::test();
    driver_character_api_v0::test();
    char_dev_handle::read_lease_self_test();
    handles::read_lease_self_test();
}
