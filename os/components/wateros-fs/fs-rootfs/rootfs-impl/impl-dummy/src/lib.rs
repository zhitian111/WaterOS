#![no_std]

//! RootFS 空实现：不持有根卷，挂载调用返回不支持。
//!
//! 用于无存储或仅编译水分支的场景；与 `impl-kernel` 互斥由特性选择。
//! 后续接入真实存储时，应换用 `impl-kernel` 并保证聚合层在 `init` 前完成 [`fs_api_v0::FsImpl`] 注入（dummy 无此路径）。
extern crate alloc;

/// 无状态的 [`api_v0::RootFsManager`] 实现。
pub struct DummyRootFsManager;

impl api_v0::RootFsManager for DummyRootFsManager {
    // 无全局槽位：忽略句柄。
    fn set_root_fs(&mut self, _fs: fs_api_v0::SharedFs) {}

    fn root_fs(&self) -> Option<fs_api_v0::SharedFs> { None }

    fn clear_root_fs(&mut self) {}

    // 不向 devfs 查询设备；恒返回不支持，避免误以为已挂载。
    fn mount_root_from_block_path(&mut self, _path: &str) -> fs_api_v0::FsResult<()> {
        Err(fs_api_v0::FsError::Unsupported)
    }

    fn current_root_device_path(&self) -> Option<alloc::string::String> { None }
}

/// 恒为 `None`，与 [`DummyRootFsManager`] 语义一致。
#[inline]
pub fn current_root_device_path() -> Option<alloc::string::String> {
    None
}
