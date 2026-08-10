#![no_std]
//! 本模块代码由AI完成

//! 内核 devfs 实现：枚举块/字符设备、维护路径绑定，并可选合并 DTB 占位节点。
//!
//! 并发边界：全局状态由 [`spin::Mutex`] 保护；[`KernelDevFsManager`] 为零大小类型，通过静态 `DEVFS` 访问。
extern crate alloc;

use alloc::{format, string::String, string::ToString, vec::Vec};
use api_v0::{DevFsManager, DevNode};
use driver_block_api_v0::{block_devices_snapshot, device_topology_generation, BlockDeviceRole,
                          SharedBlockDevice};
use driver_character_api_v0::{
    character_devices_snapshot, CharacterDeviceKind, SharedCharacterDevice,
};
use driver_input_api_v0::{evdev_character_device, input_devices_snapshot};
use fs_api_v0::{
    FsAccessMode, FsCapability, FsError, FsImpl, FsKind, FsResult, SharedFs,
};
use spin::Mutex;

#[derive(Default)]
// 本结构代码由AI完成
struct DevFsImpl {
    // 当前 refresh 后的节点快照（块/字符/占位）。
    nodes: Vec<DevNode>,
    // 路径 → 块设备句柄；refresh 会清空后按驱动枚举重建。
    block_bindings: Vec<(String, SharedBlockDevice)>,
    // 路径 → 字符设备句柄。
    character_bindings: Vec<(String, SharedCharacterDevice)>,
    // DTB 解析出的、尚无驱动实现的占位路径；在 refresh 末尾合并进 nodes。
    dt_unsupported_paths: Vec<String>,
    // 驱动注册表快照对应的 topology generation；0 表示必须重建。
    synced_generation: u64,
    // 软件节点视图代际；每次重建或直接注册都会递增。
    view_generation: u64,
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
    synced_generation: 0,
    view_generation: 0,
});

fn ensure_fresh() {
    let generation = device_topology_generation();
    if DEVFS.lock().synced_generation == generation {
        return;
    }
    let mut manager = KernelDevFsManager;
    manager.refresh();
}

// Linux 风格磁盘名：索引 0 → `/dev/vda`，超过 26 个盘符时截断到 `z`。
// 本方法代码由AI完成
fn linux_vd_disk_path(idx: usize) -> String {
    let letter = (b'a' + (idx as u8).min(25)) as char;
    format!("/dev/vd{}", letter)
}

// 同一路径不重复登记；新路径追加到 nodes 与 block_bindings。
// 本方法代码由AI完成
fn push_block_alias(inner: &mut DevFsImpl, path: String, dev: SharedBlockDevice) {
    if inner.block_bindings.iter().any(|(p, _)| p == &path) {
        return;
    }
    inner.nodes.push(api_v0::DevNode {
        path: path.clone(),
        node_type: api_v0::DevNodeType::Block,
    });
    inner.block_bindings.push((path, dev));
}

// 字符设备别名登记；逻辑同 push_block_alias。
// 本方法代码由AI完成
fn push_char_alias(inner: &mut DevFsImpl, path: String, dev: SharedCharacterDevice) {
    if inner.character_bindings.iter().any(|(p, _)| p == &path) {
        return;
    }
    inner.nodes.push(api_v0::DevNode {
        path: path.clone(),
        node_type: api_v0::DevNodeType::Character,
    });
    inner.character_bindings.push((path, dev));
}

impl DevFsManager for KernelDevFsManager {
// 本方法代码由AI完成
    fn refresh(&mut self) {
        let observed_generation = device_topology_generation();
        let block_snapshot = block_devices_snapshot();
        let char_snapshot = character_devices_snapshot();
        let input_snapshot = input_devices_snapshot();
        let dt_paths = DEVFS.lock().dt_unsupported_paths.clone();
        let block_count = block_snapshot.len();
        let char_count = char_snapshot.len();
        let input_count = input_snapshot.len();

        let mut inner = DEVFS.lock();
        inner.nodes.clear();
        inner.block_bindings.clear();
        inner.character_bindings.clear();

        for (_, dev, role) in &block_snapshot {
            if let BlockDeviceRole::Disk { disk_number } = role {
                let vd = linux_vd_disk_path(*disk_number);
                push_block_alias(&mut inner, format!("/dev/vblk{}", disk_number), dev.clone());
                push_block_alias(&mut inner, vd, dev.clone());
            }
        }
        for (_, dev, role) in &block_snapshot {
            let BlockDeviceRole::Partition { parent_device_index, partition_number } = role else {
                continue;
            };
            let Some(disk_number) = block_snapshot.iter().find_map(|(index, _, role)| {
                if index == parent_device_index {
                    if let BlockDeviceRole::Disk { disk_number } = role {
                        return Some(*disk_number);
                    }
                }
                None
            }) else {
                continue;
            };
            let path = alloc::format!("{}{}", linux_vd_disk_path(disk_number), partition_number);
            push_block_alias(&mut inner, path, dev.clone());
        }

        let mut has_console = false;
        for (idx, dev, kind) in char_snapshot {
            match kind {
                CharacterDeviceKind::Serial => {
                    push_char_alias(&mut inner, format!("/dev/ttyS{idx}"), dev.clone());
                    if !has_console {
                        push_char_alias(&mut inner, String::from("/dev/console"), dev.clone());
                        push_char_alias(&mut inner, String::from("/dev/tty"), dev.clone());
                        has_console = true;
                    }
                },
                CharacterDeviceKind::Rtc => {
                    push_char_alias(&mut inner, String::from("/dev/misc/rtc"), dev.clone());
                    push_char_alias(&mut inner, String::from("/dev/rtc0"), dev.clone());
                    push_char_alias(&mut inner, String::from("/dev/rtc"), dev.clone());
                },
                CharacterDeviceKind::Null => {
                    push_char_alias(&mut inner, String::from("/dev/null"), dev.clone());
                },
                CharacterDeviceKind::InputEvent { input_index } => {
                    push_char_alias(&mut inner, format!("/dev/input/event{input_index}"), dev.clone());
                },
            }
        }
        for (input_index, _) in input_snapshot {
            if let Ok(device) = evdev_character_device(input_index) {
                push_char_alias(&mut inner, format!("/dev/input/event{input_index}"), device);
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
        inner.synced_generation = observed_generation;
        inner.view_generation = inner.view_generation.wrapping_add(1);
        logging::info!(
            "[fs::devfs] refresh done, total_nodes={}, block={}, character={}, input={}, unsupported={}",
            inner.nodes.len(),
            block_count,
            char_count,
            input_count,
            inner.dt_unsupported_paths.len()
        );
    }

// 本方法代码由AI完成
    fn set_dt_unsupported_paths(&mut self, paths: Vec<String>) {
        let mut inner = DEVFS.lock();
        inner.dt_unsupported_paths = paths;
        inner.synced_generation = 0;
    }

// 本方法代码由AI完成
    fn list_nodes(&self) -> Vec<DevNode> {
        ensure_fresh();
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
            inner.view_generation = inner.view_generation.wrapping_add(1);
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn lookup_block_device(&self, path: &str) -> fs_api_v0::FsResult<SharedBlockDevice> {
        ensure_fresh();
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
            inner.view_generation = inner.view_generation.wrapping_add(1);
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn lookup_character_device(&self, path: &str) -> fs_api_v0::FsResult<SharedCharacterDevice> {
        ensure_fresh();
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
        ensure_fresh();
        let inner = DEVFS.lock();
        inner
            .nodes
            .iter()
            .find(|n| n.path == "/dev/vda1")
            .or_else(|| inner.nodes.iter().find(|n| n.path == "/dev/vda"))
            .or_else(|| {
                inner.nodes.iter().find(|n| {
                    matches!(n.node_type, api_v0::DevNodeType::Block)
                })
            })
            .map(|n| n.path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{boxed::Box, string::String, sync::Arc};
    use driver_input_api_v0::{register_input_device, unregister_input_device, InputDevice,
                              InputDeviceInfo, InputDeviceKind, RawInputEvent};
    struct EmptyInput(InputDeviceInfo);
    impl InputDevice for EmptyInput {
        fn info(&self) -> &InputDeviceInfo { &self.0 }
        fn pop_event(&mut self) -> driver_input_api_v0::DriverResult<Option<RawInputEvent>> {
            Ok(None)
        }
    }
    #[test]
    fn input_slot_has_stable_event_node_until_unregister() {
        let before = generation();
        let input = Arc::new(Mutex::new(Box::new(EmptyInput(InputDeviceInfo {
            name : String::from("devfs-test-input"), kind : InputDeviceKind::Keyboard,
            absolute_x : None, absolute_y : None,
        })) as Box<dyn InputDevice>));
        let index = register_input_device(input);
        let manager = KernelDevFsManager;
        let path = format!("/dev/input/event{index}");
        assert!(manager.list_nodes().iter().any(|node| node.path == path));
        assert!(generation() > before);
        assert!(manager.lookup_character_device(&path).is_ok());
        assert!(unregister_input_device(index));
        assert!(!manager.list_nodes().iter().any(|node| node.path == path));
        assert!(generation() > before);
        assert!(manager.lookup_character_device(&path).is_err());
    }
}

/// 重建 devfs 节点表并返回当前节点数量。
// 本方法代码由AI完成
pub fn refresh() -> usize {
    let mut m = KernelDevFsManager;
    m.refresh();
    m.list_nodes().len()
}

/// 返回当前软件 devfs 节点视图的代际号。
pub fn generation() -> u64 {
    ensure_fresh();
    DEVFS.lock().view_generation
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

/// 默认根块设备路径：优先真实 `/dev/vda1`，其次整盘 `/dev/vda`。
// 本方法代码由AI完成
pub fn default_root_block_path() -> Option<String> {
    let m = KernelDevFsManager;
    m.default_root_block_path()
}

/// devfs 的 [`FsImpl`] 注册项；仅列能力，不参与块卷挂载。
// 本结构代码由AI完成
pub struct KernelDevFsImpl;

/// 全局 devfs impl 实例，供聚合层 `registered_fs_impls()` 引用。
// 本变量代码由AI完成
pub static IMPL: KernelDevFsImpl = KernelDevFsImpl;

// 本变量代码由AI完成
const SUPPORTED: &[FsCapability] =
    &[FsCapability::new(FsKind::DevFs, FsAccessMode::ReadOnly)];

impl FsImpl for KernelDevFsImpl {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn supported(&self) -> &'static [FsCapability] {
        SUPPORTED
    }

// 本方法代码由AI完成
    fn mount_ro(&self, _device: SharedBlockDevice) -> FsResult<SharedFs> {
        Err(FsError::Unsupported)
    }
}
