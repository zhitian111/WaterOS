use super::*;
use super::registry::{ACTIVE_FS_IMPL, KernelRootFsManager, ROOT_DEV_PATH, ROOT_FS, ROOT_RW_FS};

pub fn set_active_fs_impl(imp: &'static dyn FsImpl) {
    *ACTIVE_FS_IMPL.lock() = Some(imp);
}

/// 返回当前注入的活动 [`FsImpl`]；未注入时为 `None`。
pub fn active_fs_impl() -> Option<&'static dyn FsImpl> { *ACTIVE_FS_IMPL.lock() }

/// 查询 `devfs` 活动实现的默认根块路径并调用 [`RootFsManager::mount_root_from_block_path`]（只读）。
// 本方法代码由AI完成
pub fn mount_default_root() -> fs_api_v0::FsResult<()> {
    let Some(path) = devfs::active_impl::default_root_block_path() else {
        return Err(fs_api_v0::FsError::NotMounted);
    };
    logging::info!("[fs::rootfs] mount default root RO from {}", path);
    let mut mgr = KernelRootFsManager;
    mgr.mount_root_from_block_path(path.as_str())
}

/// 在已注入 [`FsImpl`] 的前提下，从默认根块设备路径挂载读写根卷（bring-up 主路径）。
// 本方法代码由AI完成
pub fn mount_default_root_rw() -> fs_api_v0::FsResult<()> {
    let Some(path) = devfs::active_impl::default_root_block_path() else {
        return Err(fs_api_v0::FsError::NotMounted);
    };
    let whole_disk = mount_root_rw_from_block_path(path.as_str());
    if whole_disk.is_ok() {
        return whole_disk;
    }
    // 整盘不是文件系统（例如带分区表的整盘镜像）时，回退到首个可挂载的分区设备。
    for partition in devfs::active_impl::partition_block_paths() {
        logging::info!("[fs::rootfs] whole-disk mount failed; trying partition {}", partition);
        if let Ok(()) = mount_root_rw_from_block_path(partition.as_str()) {
            return Ok(());
        }
    }
    whole_disk
}

/// 从块设备路径挂载读写根卷并保存全局 [`SharedRwFs`]。
// 本方法代码由AI完成
pub fn mount_root_rw_from_block_path(path: &str) -> fs_api_v0::FsResult<()> {
    let device = devfs::active_impl::lookup_block_device(path)?;
    let imp = ACTIVE_FS_IMPL
        .lock()
        .ok_or(fs_api_v0::FsError::Unsupported)?;
    logging::info!("[fs::rootfs] mount root RW from {}", path);
    // VFS 修改路径使用 RW 句柄，而 ELF 装载等内核读取路径使用 RO 句柄；只发布 RW 会使
    // 后者以 `RootVolume(Unsupported)` 失败，因此两次挂载都成功后才更新全局槽。
    let root_ro = imp.mount_ro(device.clone())?;
    let root = imp.mount_rw(device)?;
    *ROOT_FS.lock() = Some(root_ro);
    *ROOT_RW_FS.lock() = Some(root);
    *ROOT_DEV_PATH.lock() = Some(path.to_string());
    bump_mount_generation();
    Ok(())
}

/// 当前根只读文件系统句柄；未挂载返回 `None`。
// 本方法代码由AI完成
pub fn root_fs() -> Option<fs_api_v0::SharedFs> {
    let mgr = KernelRootFsManager;
    mgr.root_fs()
}

/// 当前根读写文件系统句柄；未挂载返回 `None`。
pub fn root_rw_fs() -> Option<fs_api_v0::SharedRwFs> {
    ROOT_RW_FS.lock().as_ref().cloned()
}

/// 最近一次成功挂载根卷所使用的块设备路径。
// 本方法代码由AI完成
pub fn current_root_device_path() -> Option<String> {
    let mgr = KernelRootFsManager;
    mgr.current_root_device_path()
}

/// 根卷挂载代次：每次成功挂载后递增，供 VFS 页缓存失效。

/// 从块设备路径挂载 **独立** RO 卷（不替换 [`root_fs`]）。
// 本方法代码由AI完成
pub fn mount_aux_ro_from_block_path(path: &str) -> fs_api_v0::FsResult<fs_api_v0::SharedFs> {
    let device = devfs::active_impl::lookup_block_device(path)?;
    if let Some(root_path) = current_root_device_path() {
        if let Ok(root_dev) = devfs::active_impl::lookup_block_device(root_path.as_str()) {
            if Arc::ptr_eq(&device, &root_dev) {
                if let Some(root) = root_fs() {
                    logging::info!(
                        "[fs::rootfs] mount aux RO reuse root (alias {})",
                        path
                    );
                    bump_mount_generation();
                    return Ok(root);
                }
                if root_rw_fs().is_some() {
                    logging::warn!(
                        "[fs::rootfs] mount aux RO rejected: same block device as active RW root ({})",
                        path
                    );
                    return Err(fs_api_v0::FsError::Unsupported);
                }
            }
        }
    }
    let imp = ACTIVE_FS_IMPL
        .lock()
        .ok_or(fs_api_v0::FsError::Unsupported)?;
    logging::info!("[fs::rootfs] mount aux RO from {}", path);
    let aux = imp.mount_ro(device)?;
    bump_mount_generation();
    Ok(aux)
}

/// 从块设备路径挂载 **独立** RW 卷（不替换 [`root_rw_fs`]）。
// 本方法代码由AI完成
pub fn mount_aux_rw_from_block_path(path: &str) -> fs_api_v0::FsResult<fs_api_v0::SharedRwFs> {
    let device = devfs::active_impl::lookup_block_device(path)?;
    if let Some(root_path) = current_root_device_path() {
        if let Ok(root_dev) = devfs::active_impl::lookup_block_device(root_path.as_str()) {
            if Arc::ptr_eq(&device, &root_dev) {
                if let Some(root) = root_rw_fs() {
                    logging::info!(
                        "[fs::rootfs] mount aux RW reuse root (alias {})",
                        path
                    );
                    bump_mount_generation();
                    return Ok(root);
                }
            }
        }
    }
    let imp = ACTIVE_FS_IMPL
        .lock()
        .ok_or(fs_api_v0::FsError::Unsupported)?;
    logging::info!("[fs::rootfs] mount aux RW from {}", path);
    let aux = imp.mount_rw(device)?;
    bump_mount_generation();
    Ok(aux)
}
