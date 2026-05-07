#![no_std]

//! RootFS 空实现：不持有根卷，挂载调用返回不支持。
//!
//! 用于无存储或仅编译水分支的场景；与 `impl-kernel` 互斥由特性选择。
extern crate alloc;

/// 无状态的 [`api_v0::RootFsManager`] 实现。
pub struct DummyRootFsManager;

impl api_v0::RootFsManager for DummyRootFsManager {
    fn set_root_fs(&mut self, _fs: fs_api_v0::SharedFs) {}

    fn root_fs(&self) -> Option<fs_api_v0::SharedFs> { None }

    fn clear_root_fs(&mut self) {}

    fn mount_root_from_block_path(&mut self, _path: &str) -> fs_api_v0::FsResult<()> {
        Err(fs_api_v0::FsError::Unsupported)
    }

    fn current_root_device_path(&self) -> Option<alloc::string::String> { None }
}

/// 恒为 `None`，与 [`DummyRootFsManager`] 语义一致。
pub fn current_root_device_path() -> Option<alloc::string::String> {
    None
}
