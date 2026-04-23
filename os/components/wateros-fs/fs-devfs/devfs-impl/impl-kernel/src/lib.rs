#![no_std]
extern crate alloc;

use alloc::{format, string::String, string::ToString, vec::Vec};
use api_v0::{DevFsManager, DevNode};
use driver_block_api_v0::{block_device_at, block_device_count, SharedBlockDevice};
use spin::Mutex;

#[derive(Default)]
struct DevFsImpl {
    nodes: Vec<DevNode>,
    block_bindings: Vec<(String, SharedBlockDevice)>,
}

pub struct KernelDevFsManager;

static DEVFS: Mutex<DevFsImpl> = Mutex::new(DevFsImpl {
    nodes: Vec::new(),
    block_bindings: Vec::new(),
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
        logging::info!("[fs::devfs] refresh done, block_nodes={}", inner.nodes.len());
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

pub fn refresh() -> usize {
    let mut m = KernelDevFsManager;
    m.refresh();
    m.list_nodes().len()
}

pub fn list_nodes() -> Vec<DevNode> {
    let m = KernelDevFsManager;
    m.list_nodes()
}

pub fn lookup_block_device(path: &str) -> fs_api_v0::FsResult<SharedBlockDevice> {
    let m = KernelDevFsManager;
    m.lookup_block_device(path)
}

pub fn default_root_block_path() -> Option<String> {
    let m = KernelDevFsManager;
    m.default_root_block_path()
}
