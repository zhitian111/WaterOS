#![no_std]
extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use api_v0::{FsError, FsResult};
use driver_block_api_v0::{block_device_at, block_device_count, SharedBlockDevice};
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevNodeType {
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevNode {
    pub path: String,
    pub node_type: DevNodeType,
    pub index: usize,
}

static DEV_NODES: Mutex<Vec<DevNode>> = Mutex::new(Vec::new());

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
    logging::info!("[fs::devfs] refresh done, block_nodes={}", nodes.len());
    nodes.len()
}

pub fn list_nodes() -> Vec<DevNode> {
    DEV_NODES.lock().clone()
}

pub fn lookup_block_device(path: &str) -> FsResult<SharedBlockDevice> {
    let idx = parse_block_index(path).ok_or(FsError::NotFound)?;
    block_device_at(idx).ok_or(FsError::NotFound)
}

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

fn parse_block_index(path: &str) -> Option<usize> {
    let suffix = path.strip_prefix("/dev/vblk")?;
    suffix.parse::<usize>().ok()
}

