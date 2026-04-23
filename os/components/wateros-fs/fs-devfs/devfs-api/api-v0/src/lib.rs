#![no_std]
extern crate alloc;

use alloc::{string::String, vec::Vec};
use driver_block_api_v0::SharedBlockDevice;
use fs_api_v0::FsResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevNodeType {
    Block,
    Character,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevNode {
    pub path: String,
    pub node_type: DevNodeType,
}

pub trait DevFsManager {
    fn refresh(&mut self);
    fn list_nodes(&self) -> Vec<DevNode>;
    fn register_block_device(&mut self, path: &str, device: SharedBlockDevice) -> FsResult<()>;
    fn lookup_block_device(&self, path: &str) -> FsResult<SharedBlockDevice>;
    fn default_root_block_path(&self) -> Option<String>;
}
