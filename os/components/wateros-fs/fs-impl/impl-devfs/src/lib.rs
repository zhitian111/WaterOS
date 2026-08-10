#![no_std]
//! 本模块代码由AI完成

//! 用户态/测试向的简化 devfs 视图：枚举块设备为 Linux 风格 `/dev/vda*` 与兼容 `/dev/vblk{n}`。
//!
//! 与 `devfs-impl/impl-kernel` 的差异：无 DTB 占位和手工路径绑定；两者都会按驱动
//! topology generation 自动同步注册与注销。
extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use api_v0::{FsError, FsResult};
use driver_block_api_v0::{block_device_at, block_devices_snapshot, device_topology_generation,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceMetadata {
    pub major : u32,
    pub minor : u32,
    pub mode : u16,
    pub capabilities : u32,
}

const DEV_CAP_BLOCK : u32 = 1 << 0;

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

impl DevNode {
    pub fn metadata(&self) -> DeviceMetadata {
        let number = self.path.as_bytes()
                         .iter()
                         .rposition(|byte| !byte.is_ascii_digit())
                         .and_then(|index| self.path.get(index + 1..))
                         .and_then(|digits| digits.parse::<u32>().ok())
                         .unwrap_or(self.index as u32);
        match self.node_type {
            DevNodeType::Block => DeviceMetadata { major : 8,
                                                   minor : number,
                                                   mode : 0o600,
                                                   capabilities : DEV_CAP_BLOCK },
            DevNodeType::Unsupported => DeviceMetadata { major : 0,
                                                         minor : 0,
                                                         mode : 0,
                                                         capabilities : 0 },
        }
    }
}

// 与内核 devfs 不同：仅缓存枚举快照，无动态 register 表；单 Mutex 保护 bring-up 阶段并发。
// 本变量代码由AI完成
static DEV_NODES: Mutex<Vec<DevNode>> = Mutex::new(Vec::new());
static DEVFS_GENERATION : AtomicU64 = AtomicU64::new(0);

fn ensure_fresh() {
    if DEVFS_GENERATION.load(Ordering::Acquire) != device_topology_generation() {
        refresh();
    }
}

// Linux 风格磁盘名：索引 0 → `/dev/vda`。
// 本方法代码由AI完成
fn linux_vd_disk_path(index: usize) -> String {
    let letter = (b'a' + (index as u8).min(25)) as char;
    format!("/dev/vd{}", letter)
}

fn linux_vd_partition_path(disk_number : usize, partition_number : u32) -> String {
    format!("{}{}", linux_vd_disk_path(disk_number), partition_number)
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

/// 根据块设备注册表快照重建节点表并返回节点数量。
// 本方法代码由AI完成
pub fn refresh() -> usize {
    let observed_generation = device_topology_generation();
    let snapshot : Vec<_> = block_devices_snapshot()
                               .into_iter()
                               .map(|(index, _, role)| (index, role))
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
                  linux_vd_partition_path(disk_number, *partition_number),
                  *index);
    }
    logging::trace!("[fs::devfs] refresh done, block_nodes={}", nodes.len());
    DEVFS_GENERATION.store(observed_generation, Ordering::Release);
    nodes.len()
}

/// 返回当前缓存的节点列表副本。
pub fn list_nodes() -> Vec<DevNode> {
    ensure_fresh();
    DEV_NODES.lock().clone()
}

/// 将设备路径解析为索引并向驱动查询共享块设备句柄。
// 本方法代码由AI完成
pub fn lookup_block_device(path: &str) -> FsResult<SharedBlockDevice> {
    ensure_fresh();
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
    ensure_fresh();
    let nodes = DEV_NODES.lock();
    nodes.iter()
         .find(|node| node.path == "/dev/vda1")
         .or_else(|| nodes.iter().find(|node| node.path == "/dev/vda"))
         .map(|node| node.path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{boxed::Box, sync::Arc};
    use driver_block_api_v0::{BlockDevice, DriverError, DriverResult, Lba,
                              register_block_device, unregister_block_device};

    struct EmptyDisk;

    impl BlockDevice for EmptyDisk {
        fn total_blocks(&self) -> Option<u64> { Some(1) }

        fn read_blocks(&mut self, start : Lba, buf : &mut [u8]) -> DriverResult<()> {
            if start.0 != 0 || buf.len() != driver_block_api_v0::BLOCK_SIZE {
                return Err(DriverError::InvalidParam);
            }
            buf.fill(0);
            Ok(())
        }

        fn write_blocks(&mut self, _start : Lba, _buf : &[u8]) -> DriverResult<()> {
            Err(DriverError::Unsupported)
        }
    }

    #[test]
    fn lookup_lazily_tracks_registration_and_removal() {
        let device : SharedBlockDevice = Arc::new(Mutex::new(Box::new(EmptyDisk)));
        let index = register_block_device(device);
        let path = list_nodes()
            .into_iter()
            .find(|node| node.index == index && node.path.starts_with("/dev/vd"))
            .map(|node| node.path)
            .expect("new disk should appear without explicit refresh");
        assert!(lookup_block_device(path.as_str()).is_ok());
        assert!(unregister_block_device(index));
        assert!(matches!(lookup_block_device(path.as_str()), Err(FsError::NotFound)));
        assert!(!list_nodes().iter().any(|node| node.index == index));
    }

    #[test]
    fn partition_alias_keeps_wide_gpt_entry_number() {
        assert_eq!(linux_vd_partition_path(0, 300), "/dev/vda300");
        let node = DevNode { path : String::from("/dev/vda300"),
                             node_type : DevNodeType::Block,
                             index : 9 };
        assert_eq!(node.metadata().major, 8);
        assert_eq!(node.metadata().minor, 300);
        assert_eq!(node.metadata().mode, 0o600);
    }
}
