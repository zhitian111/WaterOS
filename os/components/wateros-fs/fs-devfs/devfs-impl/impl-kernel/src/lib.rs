#![no_std]

//! 内核 devfs 实现：枚举块设备、维护路径到 [`SharedBlockDevice`] 的绑定，并可选合并 DTB 占位节点。
//!
//! 并发边界：全局状态由 [`spin::Mutex`] 保护；[`KernelDevFsManager`] 为零大小类型，通过静态 `DEVFS` 访问。
extern crate alloc;

use alloc::{format, string::String, string::ToString, vec::Vec};
use api_v0::{DevFsManager, DevNode};
use driver_block_api_v0::{block_device_at, block_device_count, SharedBlockDevice};
use fs_api_v0::{
    FsAccessMode, FsCapability, FsError, FsImpl, FsKind, FsResult, SharedFs,
};
use spin::Mutex;

// 单例内核 devfs 状态：refresh 会清空并重建块绑定；DTB 路径在 refresh 时并入节点列表。
#[derive(Default)]
struct DevFsImpl {
    nodes: Vec<DevNode>,
    block_bindings: Vec<(String, SharedBlockDevice)>,
    dt_unsupported_paths: Vec<String>,
}

/// 零大小 [`DevFsManager`] 句柄；实际操作 `static DEVFS`。
pub struct KernelDevFsManager;

static DEVFS: Mutex<DevFsImpl> = Mutex::new(DevFsImpl {
    nodes: Vec::new(),
    block_bindings: Vec::new(),
    dt_unsupported_paths: Vec::new(),
});

impl DevFsManager for KernelDevFsManager {
    fn refresh(&mut self) {
        let mut inner = DEVFS.lock();
        inner.nodes.clear();
        inner.block_bindings.clear();

        let count = block_device_count();
        for idx in 0..count {
            if let Some(dev) = block_device_at(idx) {
                let path = format!("/dev/vblk{}", idx);
                inner.nodes.push(api_v0::DevNode {
                    path: path.clone(),
                    node_type: api_v0::DevNodeType::Block,
                });
                inner.block_bindings.push((path, dev));
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
            "[fs::devfs] refresh done, total_nodes={}, block={}, unsupported={}",
            inner.nodes.len(),
            count,
            inner.dt_unsupported_paths.len()
        );
    }

    fn set_dt_unsupported_paths(&mut self, paths: Vec<String>) {
        DEVFS.lock().dt_unsupported_paths = paths;
    }

    fn list_nodes(&self) -> Vec<DevNode> { DEVFS.lock().nodes.clone() }

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

    fn default_root_block_path(&self) -> Option<String> {
        DEVFS
            .lock()
            .nodes
            .iter()
            .find(|n| matches!(n.node_type, api_v0::DevNodeType::Block))
            .map(|n| n.path.clone())
    }
}

/// 刷新 devfs 并返回当前节点总数（含 DTB 占位）。
pub fn refresh() -> usize {
    let mut m = KernelDevFsManager;
    m.refresh();
    m.list_nodes().len()
}

/// 设置下一轮 [`refresh`] 将并入节点表的 DTB 路径（类型均为 Unsupported）。
pub fn set_dt_unsupported_paths(paths: Vec<String>) {
    let mut m = KernelDevFsManager;
    m.set_dt_unsupported_paths(paths);
}

/// 返回当前缓存的设备节点快照。
pub fn list_nodes() -> Vec<DevNode> {
    let m = KernelDevFsManager;
    m.list_nodes()
}

/// 在绑定表中查找路径对应的块设备句柄。
pub fn lookup_block_device(path: &str) -> fs_api_v0::FsResult<SharedBlockDevice> {
    let m = KernelDevFsManager;
    m.lookup_block_device(path)
}

/// 返回首个块设备节点路径作为默认根块路径；无块设备时 `None`。
pub fn default_root_block_path() -> Option<String> {
    let m = KernelDevFsManager;
    m.default_root_block_path()
}

/// devfs 子系统的 [`FsImpl`] 注册项。devfs 不挂在块设备上，因此 `probe`
/// 始终返回 `Ok(None)`、`mount_ro` 始终返回 [`FsError::Unsupported`]，
/// 其存在仅用于让聚合层 `supported_fs` 能列出 "内核支持 devfs" 这一事实。
pub struct KernelDevFsImpl;

pub static IMPL: KernelDevFsImpl = KernelDevFsImpl;

const SUPPORTED: &[FsCapability] =
    &[FsCapability::new(FsKind::DevFs, FsAccessMode::ReadOnly)];

impl FsImpl for KernelDevFsImpl {
    fn name(&self) -> &'static str { "devfs" }

    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }

    fn mount_ro(&self, _device: SharedBlockDevice) -> FsResult<SharedFs> {
        Err(FsError::Unsupported)
    }
}
