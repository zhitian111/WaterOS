#![no_std]

//! DevFS 空实现：无设备、无块绑定，用于未接驱动或最小链接配置。
//!
//! 语义：所有查询返回空或 [`fs_api_v0::FsError`] 中的不支持/未找到；不修改全局状态。
extern crate alloc;

use alloc::{string::String, vec::Vec};
use api_v0::{DevFsManager, DevNode};

/// 无操作的 [`DevFsManager`] 实现。
pub struct DummyDevFsManager;

impl DevFsManager for DummyDevFsManager {
    fn refresh(&mut self) {}

    fn set_dt_unsupported_paths(&mut self, _paths: Vec<String>) {}

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

/// 与内核实现 API 对齐的占位函数：丢弃 `paths`，不保留状态。
pub fn set_dt_unsupported_paths(paths: Vec<String>) {
    let mut m = DummyDevFsManager;
    m.set_dt_unsupported_paths(paths);
}
