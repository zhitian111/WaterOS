#![no_std]

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
struct DevFsImpl {
    nodes: Vec<DevNode>,
    block_bindings: Vec<(String, SharedBlockDevice)>,
    character_bindings: Vec<(String, SharedCharacterDevice)>,
    dt_unsupported_paths: Vec<String>,
}

pub struct KernelDevFsManager;

static DEVFS: Mutex<DevFsImpl> = Mutex::new(DevFsImpl {
    nodes: Vec::new(),
    block_bindings: Vec::new(),
    character_bindings: Vec::new(),
    dt_unsupported_paths: Vec::new(),
});

fn linux_vd_disk_path(idx: usize) -> String {
    let letter = (b'a' + (idx as u8).min(25)) as char;
    format!("/dev/vd{}", letter)
}

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
    fn refresh(&mut self) {
        let mut inner = DEVFS.lock();
        inner.nodes.clear();
        inner.block_bindings.clear();
        inner.character_bindings.clear();

        let count = block_device_count();
        for idx in 0..count {
            if let Some(dev) = block_device_at(idx) {
                let vd = linux_vd_disk_path(idx);
                push_block_alias(&mut inner, format!("/dev/vblk{}", idx), dev.clone());
                push_block_alias(&mut inner, vd.clone(), dev.clone());
                if idx == 0 {
                    push_block_alias(&mut inner, alloc::format!("{vd}1"), dev.clone());
                    push_block_alias(&mut inner, alloc::format!("{vd}2"), dev.clone());
                }
            }
        }

        let char_count = character_device_count();
        for idx in 0..char_count {
            let Some(dev) = character_device_at(idx) else {
                continue;
            };
            let kind = character_device_kind_at(idx).unwrap_or(CharacterDeviceKind::Serial);
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
        for path in ["/dev/zero", "/dev/urandom"] {
            if !inner.nodes.iter().any(|n| n.path == path) {
                inner.nodes.push(api_v0::DevNode {
                    path: String::from(path),
                    node_type: api_v0::DevNodeType::Character,
                });
            }
        }

        let dt_paths = inner.dt_unsupported_paths.clone();
        for path in dt_paths {
            inner.nodes.push(api_v0::DevNode {
                path,
                node_type: api_v0::DevNodeType::Unsupported,
            });
        }
        logging::info!(
            "[fs::devfs] refresh done, total_nodes={}, block={}, character={}, unsupported={}",
            inner.nodes.len(),
            count,
            char_count,
            inner.dt_unsupported_paths.len()
        );
    }

    fn set_dt_unsupported_paths(&mut self, paths: Vec<String>) {
        DEVFS.lock().dt_unsupported_paths = paths;
    }

    fn list_nodes(&self) -> Vec<DevNode> {
        DEVFS.lock().nodes.clone()
    }

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

    fn lookup_block_device(&self, path: &str) -> fs_api_v0::FsResult<SharedBlockDevice> {
        DEVFS
            .lock()
            .block_bindings
            .iter()
            .find(|(p, _)| p.as_str() == path)
            .map(|(_, dev)| dev.clone())
            .ok_or(fs_api_v0::FsError::NotFound)
    }

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

    fn lookup_character_device(&self, path: &str) -> fs_api_v0::FsResult<SharedCharacterDevice> {
        DEVFS
            .lock()
            .character_bindings
            .iter()
            .find(|(p, _)| p.as_str() == path)
            .map(|(_, dev)| dev.clone())
            .ok_or(fs_api_v0::FsError::NotFound)
    }

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

pub fn refresh() -> usize {
    let mut m = KernelDevFsManager;
    m.refresh();
    m.list_nodes().len()
}

pub fn set_dt_unsupported_paths(paths: Vec<String>) {
    let mut m = KernelDevFsManager;
    m.set_dt_unsupported_paths(paths);
}

pub fn list_nodes() -> Vec<DevNode> {
    let m = KernelDevFsManager;
    m.list_nodes()
}

pub fn lookup_block_device(path: &str) -> fs_api_v0::FsResult<SharedBlockDevice> {
    let m = KernelDevFsManager;
    m.lookup_block_device(path)
}

pub fn lookup_character_device(path: &str) -> fs_api_v0::FsResult<SharedCharacterDevice> {
    let m = KernelDevFsManager;
    m.lookup_character_device(path)
}

pub fn default_root_block_path() -> Option<String> {
    let m = KernelDevFsManager;
    m.default_root_block_path()
}

pub struct KernelDevFsImpl;

pub static IMPL: KernelDevFsImpl = KernelDevFsImpl;

const SUPPORTED: &[FsCapability] =
    &[FsCapability::new(FsKind::DevFs, FsAccessMode::ReadOnly)];

impl FsImpl for KernelDevFsImpl {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn supported(&self) -> &'static [FsCapability] {
        SUPPORTED
    }

    fn mount_ro(&self, _device: SharedBlockDevice) -> FsResult<SharedFs> {
        Err(FsError::Unsupported)
    }
}
