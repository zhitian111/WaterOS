//! devfs 设备视图同步与诊断输出。

use alloc::{string::String, vec::Vec};

use fs::devfs::active_impl as devfs_impl;

use crate::enumerate::DEVICE_INFOS;

/// 将 DTB 中未能绑定的 virtio 节点路径同步给用户态可见的 devfs 视图（具体语义由 devfs impl 定义）。
pub(crate) fn sync(unsupported_paths: Vec<String>) {
    devfs_impl::set_dt_unsupported_paths(unsupported_paths);
    let node_count = devfs_impl::refresh();
    log::info!("[driver] devfs refreshed, nodes={}", node_count);
}

/// 自检日志：依赖 `logging` 级别；不改变驱动状态。
pub(crate) fn dump_device_and_devfs_info() {
    let infos = DEVICE_INFOS.lock();
    for (idx, info) in infos.iter().enumerate() {
        log::info!(
            "[driver][test] dev#{} node={} compatible={} compatibles={:?} type={:?} mmio={:?} irq={:?}",
            idx,
            info.node_name,
            info.compatible,
            info.compatibles,
            info.device_type,
            info.mmio,
            info.irq
        );
    }
    drop(infos);

    let dev_nodes = devfs_impl::list_nodes();
    for (idx, node) in dev_nodes.iter().enumerate() {
        log::info!(
            "[driver][test] devfs-node#{} path={} type={:?}",
            idx,
            node.path,
            node.node_type
        );
    }

    let root_path = devfs_impl::default_root_block_path();
    log::info!("[driver][test] devfs default root path={:?}", root_path);
}
