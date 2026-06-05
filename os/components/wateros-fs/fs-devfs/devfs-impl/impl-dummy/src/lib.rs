#![no_std]

//! DevFS 空实现：无设备、无块/字符绑定，用于未接驱动或最小链接配置。
extern crate alloc;

use alloc::{string::String, vec::Vec};
use api_v0::{DevFsManager, DevNode};

pub struct DummyDevFsManager;

impl DevFsManager for DummyDevFsManager {
    fn refresh(&mut self) {}

    fn set_dt_unsupported_paths(&mut self, _paths: Vec<String>) {}

    fn list_nodes(&self) -> Vec<DevNode> {
        Vec::new()
    }

    fn register_block_device(
        &mut self,
        _path: &str,
        _device: driver_block_api_v0::SharedBlockDevice,
    ) -> fs_api_v0::FsResult<()> {
        Err(fs_api_v0::FsError::Unsupported)
    }

    fn lookup_block_device(
        &self,
        _path: &str,
    ) -> fs_api_v0::FsResult<driver_block_api_v0::SharedBlockDevice> {
        Err(fs_api_v0::FsError::NotFound)
    }

    fn register_character_device(
        &mut self,
        _path: &str,
        _device: driver_character_api_v0::SharedCharacterDevice,
    ) -> fs_api_v0::FsResult<()> {
        Err(fs_api_v0::FsError::Unsupported)
    }

    fn lookup_character_device(
        &self,
        _path: &str,
    ) -> fs_api_v0::FsResult<driver_character_api_v0::SharedCharacterDevice> {
        Err(fs_api_v0::FsError::NotFound)
    }

    fn default_root_block_path(&self) -> Option<String> {
        None
    }
}

pub fn set_dt_unsupported_paths(paths: Vec<String>) {
    let mut m = DummyDevFsManager;
    m.set_dt_unsupported_paths(paths);
}
