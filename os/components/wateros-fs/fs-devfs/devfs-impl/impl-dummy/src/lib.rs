#![no_std]
extern crate alloc;

use alloc::{string::String, vec::Vec};
use api_v0::{DevFsManager, DevNode};

pub struct DummyDevFsManager;

impl DevFsManager for DummyDevFsManager {
    fn refresh(&mut self) {}

    fn list_nodes(&self) -> Vec<DevNode> { Vec::new() }

    fn register_block_device(
        &mut self,
        _path: &str,
        _device: driver_block_api_v0::SharedBlockDevice,
    ) -> fs_api_v0::FsResult<()> {
        Err(fs_api_v0::FsError::Unsupported)
    }

    fn lookup_block_device(&self, _path: &str) -> fs_api_v0::FsResult<driver_block_api_v0::SharedBlockDevice> {
        Err(fs_api_v0::FsError::NotFound)
    }

    fn default_root_block_path(&self) -> Option<String> { None }
}
