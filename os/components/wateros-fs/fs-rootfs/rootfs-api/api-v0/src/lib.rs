#![no_std]

//! RootFS 管理 API（v0）：当前根只读卷句柄与根块设备路径的存取契约。
//!
//! 挂载语义由实现配合 devfs 与已注入的 [`fs_api_v0::FsImpl`] 完成；本 trait 不规定卷格式。
extern crate alloc;

use alloc::string::String;
use fs_api_v0::{FsResult, SharedFs};

/// 根卷管理：设置/查询当前根 [`SharedFs`]，以及从块设备路径挂载。
pub trait RootFsManager {
    /// 设置当前根文件系统句柄（通常由 `mount_root_from_block_path` 内部调用）。
    fn set_root_fs(&mut self, fs: SharedFs);
    /// 返回当前根卷句柄；未挂载时为 `None`。
    fn root_fs(&self) -> Option<SharedFs>;
    /// 清除根卷句柄与关联的根设备路径（若实现维护该路径）。
    fn clear_root_fs(&mut self);
    /// 从 devfs 解析 `path` 对应块设备并以当前活动 [`fs_api_v0::FsImpl`] 执行 RO 挂载。
    fn mount_root_from_block_path(&mut self, path: &str) -> FsResult<()>;
    /// 最近一次成功挂载根卷所使用的块设备路径。
    fn current_root_device_path(&self) -> Option<String>;
}
