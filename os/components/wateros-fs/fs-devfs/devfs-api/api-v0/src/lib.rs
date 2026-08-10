#![no_std]

//! DevFS 管理 API（v0）：设备节点类型、枚举与块设备注册/查找契约。
//!
//! 与 [`fs_api_v0::FsImpl`] 解耦：本模块描述「设备树视图」，不描述具体卷文件系统。
//!
//! 错误类型复用 [`fs_api_v0::FsResult`] / [`fs_api_v0::FsError`]，便于与根卷挂载路径统一映射。
extern crate alloc;

use alloc::{string::String, vec::Vec};
use driver_block_api_v0::SharedBlockDevice;
use driver_character_api_v0::SharedCharacterDevice;
use fs_api_v0::FsResult;

/// devfs 中节点的粗分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevNodeType {
    /// 块设备节点（可参与根卷挂载）。
    Block,
    /// 字符设备（预留；当前部分实现可能未填充）。
    Character,
    /// DTB 等设备已枚举，但当前内核无对应子系统实现或未成功绑定。
    Unsupported,
}

/// 单条设备节点：路径与类型，用于启动日志与块设备路径解析。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevNode {
    /// 逻辑路径（如 `/dev/vblk0`）。
    pub path: String,
    /// 节点类型。
    pub node_type: DevNodeType,
    /// Linux-style major number when the platform driver provides one.
    pub major: Option<u32>,
    /// Linux-style minor number when the platform driver provides one.
    pub minor: Option<u32>,
    /// Node permission bits (file type bits are kept outside this view).
    pub mode: u16,
}

/// DevFS 管理器：刷新节点表、登记 DTB 占位路径、注册/查找块设备。
pub trait DevFsManager {
    /// 根据当前平台驱动枚举重建节点表（实现可清空并重建绑定）。
    fn refresh(&mut self);
    /// 在 [`refresh`](Self::refresh) 使用的块设备表之外，登记仅作占位展示的 DTB 节点路径（类型均为 [`DevNodeType::Unsupported`]）。
    fn set_dt_unsupported_paths(&mut self, paths: Vec<String>);
    /// 返回当前缓存的节点快照（顺序由实现决定）。
    fn list_nodes(&self) -> Vec<DevNode>;
    /// 将块设备绑定到给定路径；已存在路径时语义由实现定义（内核实现为替换）。
    fn register_block_device(&mut self, path: &str, device: SharedBlockDevice) -> FsResult<()>;
    /// 按路径查找已注册的块设备句柄。
    fn lookup_block_device(&self, path: &str) -> FsResult<SharedBlockDevice>;
    /// 将字符设备绑定到给定路径。
    fn register_character_device(
        &mut self,
        path: &str,
        device: SharedCharacterDevice,
    ) -> FsResult<()>;
    /// 按路径查找已注册的字符设备句柄。
    fn lookup_character_device(&self, path: &str) -> FsResult<SharedCharacterDevice>;
    /// 默认用于根卷探测的块设备路径；无可用设备时返回 `None`。
    fn default_root_block_path(&self) -> Option<String>;
}
