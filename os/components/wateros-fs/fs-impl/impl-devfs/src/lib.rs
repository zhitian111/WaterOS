#![no_std]
//! 本模块代码由AI完成

//! 用户态/测试向的简化 devfs 视图：枚举块设备为 Linux 风格 `/dev/vda*` 与兼容 `/dev/vblk{n}`。
//!
//! 与 `devfs-impl/impl-kernel` 的差异：无 DTB 占位、无动态 `register_block_device`；路径解析规则见 `parse_block_index`（模块内私有）。
extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use api_v0::{FsError, FsResult};
use driver_block_api_v0::{block_device_at, block_device_count, block_device_role_at,
                          BlockDeviceRole, SharedBlockDevice};
use spin::Mutex;

/// 简化 devfs 中的节点类型（仅块与占位未使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// 本结构代码由AI完成
pub enum DevNodeType {
    /// 块设备索引节点。
    Block,
    /// 预留；当前刷新逻辑不生成此变体。
    Unsupported,
}

/// 带块设备索引的节点描述，便于测试断言。
#[derive(Debug, Clone, PartialEq, Eq)]
// 本结构代码由AI完成
pub struct DevNode {
    /// 逻辑路径。
    pub path: String,
    /// 节点类型。
    pub node_type: DevNodeType,
    /// 在驱动枚举中的块设备索引。
    pub index: usize,
}

// 与内核 devfs 不同：仅缓存枚举快照，无动态 register 表；单 Mutex 保护 bring-up 阶段并发。
// 本变量代码由AI完成
static DEV_NODES: Mutex<Vec<DevNode>> = Mutex::new(Vec::new());

// Linux 风格磁盘名：索引 0 → `/dev/vda`。
// 本方法代码由AI完成
fn linux_vd_disk_path(index: usize) -> String {
    let letter = (b'a' + (index as u8).min(25)) as char;
    format!("/dev/vd{}", letter)
}

// 本方法代码由AI完成
fn push_node(nodes: &mut Vec<DevNode>, path: String, index: usize) {
    if nodes.iter().any(|n| n.path == path) {
        return;
    }
    nodes.push(DevNode {
        path,
        node_type: DevNodeType::Block,
        index,
    });
}

/// 根据 `block_device_count()` 重建节点表并返回节点数量。
// 本方法代码由AI完成
pub fn refresh() -> usize {
    let count = block_device_count();
    let snapshot : Vec<_> = (0..count).filter_map(|index| {
                                             block_device_role_at(index).map(|role| (index, role))
                                         })
                                         .collect();
    let mut nodes = DEV_NODES.lock();
    nodes.clear();
    for (index, role) in &snapshot {
        if let BlockDeviceRole::Disk { disk_number } = role {
            push_node(&mut nodes, format!("/dev/vblk{}", disk_number), *index);
            push_node(&mut nodes, linux_vd_disk_path(*disk_number), *index);
        }
    }
    for (index, role) in &snapshot {
        let BlockDeviceRole::Partition { parent_device_index, partition_number } = role else {
            continue;
        };
        let Some(disk_number) = snapshot.iter().find_map(|(candidate, role)| {
            if candidate == parent_device_index {
                if let BlockDeviceRole::Disk { disk_number } = role {
                    return Some(*disk_number);
                }
            }
            None
        }) else {
            continue;
        };
        push_node(&mut nodes,
                  format!("{}{}", linux_vd_disk_path(disk_number), partition_number),
                  *index);
    }
    logging::trace!("[fs::devfs] refresh done, block_nodes={}", nodes.len());
    nodes.len()
}

/// 返回当前缓存的节点列表副本。
pub fn list_nodes() -> Vec<DevNode> {
    DEV_NODES.lock().clone()
}

/// 将设备路径解析为索引并向驱动查询共享块设备句柄。
// 本方法代码由AI完成
pub fn lookup_block_device(path: &str) -> FsResult<SharedBlockDevice> {
    let idx = DEV_NODES.lock()
                       .iter()
                       .find(|node| node.path == path)
                       .map(|node| node.index)
                       .ok_or(FsError::NotFound)?;
    block_device_at(idx).ok_or(FsError::NotFound)
}

/// 优先返回第一个真实分区，无分区时回退到整盘。
// 本方法代码由AI完成
pub fn default_root_block_path() -> Option<String> {
    let nodes = DEV_NODES.lock();
    nodes.iter()
         .find(|node| node.path == "/dev/vda1")
         .or_else(|| nodes.iter().find(|node| node.path == "/dev/vda"))
         .map(|node| node.path.clone())
}
