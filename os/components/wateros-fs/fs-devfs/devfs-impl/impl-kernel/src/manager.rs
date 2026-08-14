use super::*;

#[path = "aliases.rs"]
mod aliases;
pub(crate) use aliases::linux_vd_disk_path;
use aliases::{linux_vd_partition_path, push_block_alias, push_char_alias};

#[derive(Default)]
// 本结构代码由AI完成
pub(crate) struct DevFsImpl {
    // 当前 refresh 后的节点快照（块/字符/占位）。
    pub(crate) nodes: Vec<DevNode>,
    // 路径 → 块设备句柄；refresh 会清空后按驱动枚举重建。
    pub(crate) block_bindings: Vec<(String, SharedBlockDevice)>,
    // 路径 → 字符设备句柄。
    pub(crate) character_bindings: Vec<(String, SharedCharacterDevice)>,
    // DTB 解析出的、尚无驱动实现的占位路径；在 refresh 末尾合并进 nodes。
    pub(crate) dt_unsupported_paths: Vec<String>,
}

/// 零大小 [`DevFsManager`] 句柄；实际状态在静态 `DEVFS` 中。
// 本结构代码由AI完成
pub struct KernelDevFsManager;

// 本变量代码由AI完成
static DEVFS: Mutex<DevFsImpl> = Mutex::new(DevFsImpl {
    nodes: Vec::new(),
    block_bindings: Vec::new(),
    character_bindings: Vec::new(),
    dt_unsupported_paths: Vec::new(),
});

// Linux 风格磁盘名：索引 0 → `/dev/vda`，超过 26 个盘符时截断到 `z`。
// 本方法代码由AI完成
impl DevFsManager for KernelDevFsManager {
// 本方法代码由AI完成
    fn refresh(&mut self) {
        let block_snapshot = block_devices_snapshot();
        let char_snapshot: alloc::vec::Vec<_> = (0..character_device_count())
            .filter_map(|idx| {
                character_device_at(idx).map(|dev| {
                    (
                        idx,
                        dev,
                        character_device_kind_at(idx).unwrap_or(CharacterDeviceKind::Serial),
                    )
                })
            })
            .collect();
        let dt_paths = DEVFS.lock().dt_unsupported_paths.clone();
        let block_count = block_snapshot.len();
        let char_count = char_snapshot.len();

        let mut inner = DEVFS.lock();
        inner.nodes.clear();
        inner.block_bindings.clear();
        inner.character_bindings.clear();

        for (idx, dev, role) in &block_snapshot {
            match role {
                BlockDeviceRole::Disk { disk_number } => {
                    let vd = linux_vd_disk_path(*disk_number);
                    push_block_alias(&mut inner, format!("/dev/vblk{}", idx), dev.clone());
                    push_block_alias(&mut inner, vd.clone(), dev.clone());
                }
                BlockDeviceRole::Partition { parent_device_index, partition_number } => {
                    let disk_number = block_snapshot.iter().find_map(|(index, _, role)| {
                        if *index == *parent_device_index {
                            if let BlockDeviceRole::Disk { disk_number } = role {
                                return Some(*disk_number);
                            }
                        }
                        None
                    });
                    if let Some(disk_number) = disk_number {
                        let path = linux_vd_partition_path(disk_number, *partition_number);
                        push_block_alias(&mut inner, path, dev.clone());
                    }
                }
            }
        }

        for (idx, dev, kind) in char_snapshot {
            push_char_alias(&mut inner, format!("/dev/ttyS{idx}"), dev.clone());
            if idx == 0 {
                push_char_alias(&mut inner, String::from("/dev/console"), dev.clone());
                push_char_alias(&mut inner, String::from("/dev/tty"), dev.clone());
            }
            if kind == CharacterDeviceKind::Rtc {
                push_char_alias(&mut inner, String::from("/dev/misc/rtc"), dev.clone());
                push_char_alias(&mut inner, String::from("/dev/rtc0"), dev.clone());
                push_char_alias(&mut inner, String::from("/dev/rtc"), dev.clone());
            }
            if kind == CharacterDeviceKind::Null {
                push_char_alias(&mut inner, String::from("/dev/null"), dev.clone());
            }
        }
        for path in ["/dev/null", "/dev/zero", "/dev/urandom", "/dev/cpu_dma_latency"] {
            if !inner.nodes.iter().any(|n| n.path == path) {
                inner.nodes.push(api_v0::DevNode {
                    path: String::from(path),
                    node_type: api_v0::DevNodeType::Character,
                });
            }
        }

        for path in dt_paths {
            inner.nodes.push(api_v0::DevNode {
                path,
                node_type: api_v0::DevNodeType::Unsupported,
            });
        }
        logging::info!(
            "[fs::devfs] refresh done, total_nodes={}, block={}, character={}, unsupported={}",
            inner.nodes.len(),
            block_count,
            char_count,
            inner.dt_unsupported_paths.len()
        );
    }

// 本方法代码由AI完成
    fn set_dt_unsupported_paths(&mut self, paths: Vec<String>) {
        DEVFS.lock().dt_unsupported_paths = paths;
    }

// 本方法代码由AI完成
    fn list_nodes(&self) -> Vec<DevNode> {
        DEVFS.lock().nodes.clone()
    }

// 本方法代码由AI完成
    fn register_block_device(
        &mut self,
        path: &str,
        device: SharedBlockDevice,
    ) -> fs_api_v0::FsResult<()> {
        let mut inner = DEVFS.lock();
        if let Some((_, dev)) = inner
            .block_bindings
            .iter_mut()
            .find(|(p, _)| p.as_str() == path)
        {
            *dev = device;
        } else {
            inner.block_bindings.push((path.to_string(), device));
            inner.nodes.push(api_v0::DevNode {
                path: path.to_string(),
                node_type: api_v0::DevNodeType::Block,
            });
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn lookup_block_device(&self, path: &str) -> fs_api_v0::FsResult<SharedBlockDevice> {
        DEVFS
            .lock()
            .block_bindings
            .iter()
            .find(|(p, _)| p.as_str() == path)
            .map(|(_, dev)| dev.clone())
            .ok_or(fs_api_v0::FsError::NotFound)
    }

// 本方法代码由AI完成
    fn register_character_device(
        &mut self,
        path: &str,
        device: SharedCharacterDevice,
    ) -> fs_api_v0::FsResult<()> {
        let mut inner = DEVFS.lock();
        if let Some((_, dev)) = inner
            .character_bindings
            .iter_mut()
            .find(|(p, _)| p.as_str() == path)
        {
            *dev = device;
        } else {
            inner.character_bindings.push((path.to_string(), device));
            inner.nodes.push(api_v0::DevNode {
                path: path.to_string(),
                node_type: api_v0::DevNodeType::Character,
            });
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn lookup_character_device(&self, path: &str) -> fs_api_v0::FsResult<SharedCharacterDevice> {
        DEVFS
            .lock()
            .character_bindings
            .iter()
            .find(|(p, _)| p.as_str() == path)
            .map(|(_, dev)| dev.clone())
            .ok_or(fs_api_v0::FsError::NotFound)
    }

// 本方法代码由AI完成
    fn default_root_block_path(&self) -> Option<String> {
        let inner = DEVFS.lock();
        inner
            .nodes
            .iter()
            .find(|n| n.path == "/dev/vda")
            .or_else(|| {
                inner.nodes.iter().find(|n| {
                    matches!(n.node_type, api_v0::DevNodeType::Block)
                })
            })
            .map(|n| n.path.clone())
    }
}

/// 重建 devfs 节点表并返回当前节点数量。
// 本方法代码由AI完成
pub fn refresh() -> usize {
    let mut m = KernelDevFsManager;
    m.refresh();
    m.list_nodes().len()
}

/// 登记 DTB 占位路径（下次 [`refresh`] 时合并进节点表）。
// 本方法代码由AI完成
pub fn set_dt_unsupported_paths(paths: Vec<String>) {
    let mut m = KernelDevFsManager;
    m.set_dt_unsupported_paths(paths);
}

/// 返回当前节点列表快照。
// 本方法代码由AI完成
pub fn list_nodes() -> Vec<DevNode> {
    let m = KernelDevFsManager;
    m.list_nodes()
}

/// 按路径查找块设备句柄。
// 本方法代码由AI完成
pub fn lookup_block_device(path: &str) -> fs_api_v0::FsResult<SharedBlockDevice> {
    let m = KernelDevFsManager;
    m.lookup_block_device(path)
}

/// 按路径查找字符设备句柄。
// 本方法代码由AI完成
pub fn lookup_character_device(path: &str) -> fs_api_v0::FsResult<SharedCharacterDevice> {
    let m = KernelDevFsManager;
    m.lookup_character_device(path)
}

/// 默认根块设备路径：优先 `/dev/vda`，否则取首个块节点。
// 本方法代码由AI完成
pub fn default_root_block_path() -> Option<String> {
    let m = KernelDevFsManager;
    m.default_root_block_path()
}
