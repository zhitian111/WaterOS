//! 2K1000LA 启动期 devfs 视图同步。

/// Refresh the active devfs implementation after platform character devices
/// have been registered. The operation is idempotent and only rebuilds the
/// software view; it does not probe or mutate hardware.
pub(crate) fn sync() {
    let node_count = fs::devfs::active_impl::refresh();
    log::info!("[driver-ls2k][devfs] refreshed after UART registration, nodes={}",
               node_count);
}
