//! per-task fd 表与控制台/pipe [`VfsIoHandle`] 实现。
//! 本模块代码由AI完成

#![no_std]

extern crate alloc;

pub mod char_dev_handle;
pub mod cwd;
pub mod file_lock;
pub mod handles;
pub mod interrupt_guard;
#[cfg(feature = "user-graphics")]
mod pty;
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
pub use user_graphics::{initialize_user_graphics_devices, user_graphics_input_worker};

#[cfg(not(feature = "user-graphics"))]
pub fn initialize_user_graphics_devices() -> bool { false }

#[cfg(not(feature = "user-graphics"))]
pub extern "C" fn user_graphics_input_worker(_arg : usize) -> ! {
    loop { task::sleep_for_ticks(1); }
}

pub fn special_device_exists(path : &str) -> bool {
    {
        #[cfg(feature = "user-graphics")]
        if pty::pty_special_device_exists(path) { return true; }
        #[cfg(feature = "user-graphics")]
        { user_graphics::special_device_exists(path) }
        #[cfg(not(feature = "user-graphics"))]
        { let _ = path; false }
    }
}

pub fn special_device_metadata(path : &str) -> Option<api_v0::VfsMetadata> {
    #[cfg(feature = "user-graphics")]
    if let Some(metadata) = pty::pty_special_device_metadata(path) { return Some(metadata); }
    #[cfg(feature = "user-graphics")]
    { user_graphics::special_device_metadata(path) }
    #[cfg(not(feature = "user-graphics"))]
    { let _ = path; None }
}

pub fn special_device_paths() -> alloc::vec::Vec<alloc::string::String> {
    #[cfg(feature = "user-graphics")]
    {
        let mut paths = pty::pty_special_device_paths();
        paths.extend(user_graphics::special_device_paths());
        paths
    }
    #[cfg(not(feature = "user-graphics"))]
    { alloc::vec::Vec::new() }
}

pub fn open_special_device(
    path : &str,
    accmode : u32,
    nonblocking : bool,
) -> Option<api_v0::VfsResult<alloc::boxed::Box<dyn api_v0::VfsIoHandle>>> {
    #[cfg(feature = "user-graphics")]
    {
        let sid = task::current_process_snapshot().map(|process| process.sid.raw());
        if let Some(opened) = pty::open_pty_special_device(path, accmode, nonblocking, sid) {
            return Some(opened);
        }
        user_graphics::open_special_device(path, accmode, nonblocking)
    }
    #[cfg(not(feature = "user-graphics"))]
    { let _ = (path, accmode, nonblocking); None }
}

pub fn pty_endpoint_for_handle(handle: &(dyn api_v0::VfsIoHandle + '_))
                               -> Option<tty::PtyEndpointHandle> {
    #[cfg(feature = "user-graphics")]
    {
        handle.as_any().downcast_ref::<pty::PtyVfsHandle>()
              .map(|handle| handle.endpoint().clone())
    }
    #[cfg(not(feature = "user-graphics"))]
    { let _ = handle; None }
}

pub fn test() {
    ipc::pipe::test();
    driver_character_api_v0::test();
    char_dev_handle::read_lease_self_test();
    handles::read_lease_self_test();
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    test();
}
