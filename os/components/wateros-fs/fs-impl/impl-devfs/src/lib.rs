#![no_std]
//! 本模块代码由AI完成

//! 用户态/测试向的简化 devfs 视图：枚举块设备为 Linux 风格 `/dev/vda*` 与兼容 `/dev/vblk{n}`。
//!
//! 与 `devfs-impl/impl-kernel` 的差异：无 DTB 占位、无动态 `register_block_device`；路径解析规则见 `parse_block_index`（模块内私有）。
extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use api_v0::*;
use driver_block_api_v0::{block_device_at, block_device_count, SharedBlockDevice};
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

#[cfg(feature = "self_test")]
pub fn self_test() {
    logging::info!("[fs/devfs] self_test begin");
    assert_eq!(linux_vd_disk_path(0), "/dev/vda");
    assert_eq!(linux_vd_disk_path(1), "/dev/vdb");
    logging::info!("[fs/devfs] self_test complete");
}

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
    let mut nodes = DEV_NODES.lock();
    nodes.clear();
    for idx in 0..count {
        let vd = linux_vd_disk_path(idx);
        push_node(&mut nodes, format!("/dev/vblk{}", idx), idx);
        push_node(&mut nodes, vd.clone(), idx);
        if idx == 0 {
            push_node(&mut nodes, format!("{vd}1"), idx);
            push_node(&mut nodes, format!("{vd}2"), idx);
        }
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
    let idx = parse_block_index(path).ok_or(FsError::NotFound)?;
    block_device_at(idx).ok_or(FsError::NotFound)
}

/// 存在至少一块设备时返回 `/dev/vda`，否则 `None`。
// 本方法代码由AI完成
pub fn default_root_block_path() -> Option<String> {
    if block_device_count() == 0 {
        None
    } else {
        Some(linux_vd_disk_path(0))
    }
}

// 路径格式与 impl-kernel 命名保持一致，便于测试共享镜像。
// 本方法代码由AI完成
fn parse_block_index(path: &str) -> Option<usize> {
    if let Some(suffix) = path.strip_prefix("/dev/vblk") {
        return suffix.parse::<usize>().ok();
    }
    if let Some(rest) = path.strip_prefix("/dev/vd") {
        let mut chars = rest.chars();
        let disk_letter = chars.next()?;
        if !disk_letter.is_ascii_lowercase() {
            return None;
        }
        let disk_idx = (disk_letter as u8 - b'a') as usize;
        let part: String = chars.collect();
        if part.is_empty() || part.chars().all(|c| c.is_ascii_digit()) {
            return Some(disk_idx);
        }
        return None;
    }
    None
}
