#![no_std]
extern crate alloc;

use alloc::string::String;
use fs_api_v0::{FsResult, SharedFs};

pub trait RootFsManager {
    fn set_root_fs(&mut self, fs: SharedFs);
    fn root_fs(&self) -> Option<SharedFs>;
    fn clear_root_fs(&mut self);
    fn mount_root_from_block_path(&mut self, path: &str) -> FsResult<()>;
    fn current_root_device_path(&self) -> Option<String>;
}
