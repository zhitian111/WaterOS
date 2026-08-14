//! 本模块代码由AI完成

//! 内核 devfs 实现：枚举块/字符设备、维护路径绑定，并可选合并 DTB 占位节点。
//!
//! 并发边界：全局状态由 [`spin::Mutex`] 保护；[`KernelDevFsManager`] 为零大小类型，通过静态 `DEVFS` 访问。
extern crate alloc;

use alloc::{format, string::String, string::ToString, vec::Vec};
use api_v0::{DevFsManager, DevNode};
use driver_block_api_v0::{block_device_at, block_device_count, SharedBlockDevice};
use driver_character_api_v0::{
    character_device_at, character_device_count, character_device_kind_at,
    CharacterDeviceKind, SharedCharacterDevice,
};
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
        let block_snapshot: alloc::vec::Vec<_> = (0..block_device_count())
            .filter_map(|idx| block_device_at(idx).map(|dev| (idx, dev)))
            .collect();
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

        for (idx, dev) in block_snapshot {
            let vd = linux_vd_disk_path(idx);
            push_block_alias(&mut inner, format!("/dev/vblk{}", idx), dev.clone());
            push_block_alias(&mut inner, vd.clone(), dev.clone());
            if idx == 0 {
                push_block_alias(&mut inner, alloc::format!("{vd}1"), dev.clone());
                push_block_alias(&mut inner, alloc::format!("{vd}2"), dev.clone());
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

/// devfs 的 [`FsImpl`] 注册项；仅列能力，不参与块卷挂载。
// 本结构代码由AI完成
pub struct KernelDevFsImpl;

/// 全局 devfs impl 实例，供聚合层 `registered_fs_impls()` 引用。
// 本变量代码由AI完成
pub static IMPL: KernelDevFsImpl = KernelDevFsImpl;

#[cfg(feature = "self_test")]
pub fn self_test() {
    logging::info!("[fs/devfs/impl-kernel] self_test begin");
    assert_eq!(linux_vd_disk_path(0), "/dev/vda");
    assert_eq!(linux_vd_disk_path(25), "/dev/vdz");
    logging::info!("[fs/devfs/impl-kernel] self_test complete");
}

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
