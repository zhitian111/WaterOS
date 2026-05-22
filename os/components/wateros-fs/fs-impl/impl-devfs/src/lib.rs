#![no_std]

//! 用户态/测试向的简化 devfs 视图：仅枚举 `driver_block_api_v0` 已注册的块设备为 `/dev/vblk{n}`。
//!
//! 与 `devfs-impl/impl-kernel` 的差异：无 DTB 占位、无动态 `register_block_device`；路径解析规则见 `parse_block_index`（模块内私有）。
extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use api_v0::{FsError, FsResult};
use driver_block_api_v0::{block_device_at, block_device_count, SharedBlockDevice};
use spin::Mutex;

/// 简化 devfs 中的节点类型（仅块与占位未使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevNodeType {
    /// 块设备索引节点。
    Block,
    /// 预留；当前刷新逻辑不生成此变体。
    Unsupported,
}

/// 带块设备索引的节点描述，便于测试断言。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevNode {
    /// 逻辑路径。
    pub path: String,
    /// 节点类型。
    pub node_type: DevNodeType,
    /// 在驱动枚举中的块设备索引。
    pub index: usize,
}

// 与内核 devfs 不同：仅缓存枚举快照，无动态 register 表；单 Mutex 保护 bring-up 阶段并发。
static DEV_NODES: Mutex<Vec<DevNode>> = Mutex::new(Vec::new());

/// 根据 `block_device_count()` 重建节点表并返回节点数量。
pub fn refresh() -> usize {
    let count = block_device_count();
    let mut nodes = DEV_NODES.lock();
    nodes.clear();
    for idx in 0..count {
        nodes.push(DevNode {
            path: make_block_path(idx),
            node_type: DevNodeType::Block,
            index: idx,
        });
    }
    logging::trace!("[fs::devfs] refresh done, block_nodes={}", nodes.len());
    nodes.len()
}

/// 返回当前缓存的节点列表副本。
pub fn list_nodes() -> Vec<DevNode> {
    DEV_NODES.lock().clone()
}

/// 将 `/dev/vblk{n}` 解析为索引并向驱动查询共享块设备句柄。
pub fn lookup_block_device(path: &str) -> FsResult<SharedBlockDevice> {
    let idx = parse_block_index(path).ok_or(FsError::NotFound)?;
    block_device_at(idx).ok_or(FsError::NotFound)
}

/// 存在至少一块设备时返回 `/dev/vblk0`，否则 `None`。
pub fn default_root_block_path() -> Option<String> {
    if block_device_count() == 0 {
        None
    } else {
        Some(make_block_path(0))
    }
}

fn make_block_path(index: usize) -> String {
    format!("/dev/vblk{}", index)
}

// 路径格式与 impl-kernel 默认命名保持一致，便于测试共享镜像。
fn parse_block_index(path: &str) -> Option<usize> {
    let suffix = path.strip_prefix("/dev/vblk")?;
    suffix.parse::<usize>().ok()
}
