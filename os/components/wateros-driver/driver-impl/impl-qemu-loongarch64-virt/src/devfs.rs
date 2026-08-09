//! devfs 设备视图同步。

use fs::devfs::active_impl as devfs_impl;

/// 刷新 devfs 视图，填充 `/dev/sys/*`。
pub(crate) fn sync() {
    let node_count = devfs_impl::refresh();
    log::info!(
        "[driver-la] devfs refreshed, nodes={}",
        node_count
    );
}
