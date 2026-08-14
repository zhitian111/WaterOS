//! Linux device-node naming and alias registration.

extern crate alloc;
use alloc::{format, string::String};
use api_v0::{DevNode, DevNodeType};
use driver_block_api_v0::SharedBlockDevice;
use driver_character_api_v0::SharedCharacterDevice;
use super::DevFsImpl;

pub(crate) fn linux_vd_disk_path(idx: usize) -> String {
    let letter = (b'a' + (idx as u8).min(25)) as char;
    format!("/dev/vd{}", letter)
}

pub(crate) fn linux_vd_partition_path(disk_number : usize, partition_number : u32) -> String {
    format!("{}{}", linux_vd_disk_path(disk_number), partition_number)
}

pub(crate) fn push_block_alias(inner: &mut DevFsImpl, path: String, dev: SharedBlockDevice) {
    if inner.block_bindings.iter().any(|(p, _)| p == &path) { return; }
    inner.nodes.push(DevNode { path: path.clone(), node_type: DevNodeType::Block });
    inner.block_bindings.push((path, dev));
}

pub(crate) fn push_char_alias(inner: &mut DevFsImpl, path: String, dev: SharedCharacterDevice) {
    if inner.character_bindings.iter().any(|(p, _)| p == &path) { return; }
    inner.nodes.push(DevNode { path: path.clone(), node_type: DevNodeType::Character });
    inner.character_bindings.push((path, dev));
}
