#![no_std]
//! 本模块代码由AI完成

//! 内核 rootfs 实现：静态槽位保存当前根 [`SharedFs`]、根块路径与聚合层注入的 [`FsImpl`]。
//!
//! 挂载前须由外层调用 [`set_active_fs_impl`]；否则 [`mount_default_root`] 与 [`RootFsManager::mount_root_from_block_path`] 将失败。
extern crate alloc;

use alloc::string::{String, ToString};
use api_v0::RootFsManager;
use core::sync::atomic::{AtomicU64, Ordering};
use alloc::sync::Arc;
use fs_api_v0::FsImpl;
use spin::Mutex;

// 本变量代码由AI完成
static MOUNT_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 零大小 [`RootFsManager`] 句柄；读写 `static ROOT_FS` 等状态。
// 本结构代码由AI完成
pub struct KernelRootFsManager;

// 本变量代码由AI完成
static ROOT_FS: Mutex<Option<fs_api_v0::SharedFs>> = Mutex::new(None);
// 本变量代码由AI完成
static ROOT_RW_FS: Mutex<Option<fs_api_v0::SharedRwFs>> = Mutex::new(None);
// 本变量代码由AI完成
static ROOT_DEV_PATH: Mutex<Option<String>> = Mutex::new(None);
// 生命周期：'static 引用指向注册表中的 impl，由链接期/聚合 init 保证长于内核运行期。
/// 由聚合层在启动期注入的「当前根 FS impl」。聚合层根据 `probe` 选定后调用 [`set_active_fs_impl`]。
// 本变量代码由AI完成
static ACTIVE_FS_IMPL: Mutex<Option<&'static dyn FsImpl>> = Mutex::new(None);

impl api_v0::RootFsManager for KernelRootFsManager {
// 本方法代码由AI完成
    fn set_root_fs(&mut self, fs: fs_api_v0::SharedFs) {
        *ROOT_FS.lock() = Some(fs);
    }

    // 克隆 Arc：调用方获得独立句柄，与槽位内实例共享底层卷状态。
    fn root_fs(&self) -> Option<fs_api_v0::SharedFs> { ROOT_FS.lock().as_ref().cloned() }

// 本方法代码由AI完成
    fn clear_root_fs(&mut self) {
        *ROOT_FS.lock() = None;
        *ROOT_RW_FS.lock() = None;
        *ROOT_DEV_PATH.lock() = None;
    }

// 本方法代码由AI完成
    fn mount_root_from_block_path(&mut self, path: &str) -> fs_api_v0::FsResult<()> {
        // devfs 解析路径 → 活动 FsImpl RO 挂载 → 记录路径，三步任一失败即向上返回。
        let device = devfs::active_impl::lookup_block_device(path)?;
        let imp = ACTIVE_FS_IMPL
            .lock()
            .ok_or(fs_api_v0::FsError::Unsupported)?;
        let root = imp.mount_ro(device)?;
        self.set_root_fs(root);
        *ROOT_DEV_PATH.lock() = Some(path.to_string());
        MOUNT_GENERATION.fetch_add(1, Ordering::Release);
        Ok(())
    }

    fn current_root_device_path(&self) -> Option<String> { ROOT_DEV_PATH.lock().as_ref().cloned() }
}

/// 由聚合层注入选好的 FS impl（按 `FsImpl::probe` 与 `supports(kind, ReadOnly)` 选取）。
// 本方法代码由AI完成
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
    mount_root_rw_from_block_path(path.as_str())
}

/// 从块设备路径挂载读写根卷并保存全局 [`SharedRwFs`]。
// 本方法代码由AI完成
pub fn mount_root_rw_from_block_path(path: &str) -> fs_api_v0::FsResult<()> {
    let device = devfs::active_impl::lookup_block_device(path)?;
    let imp = ACTIVE_FS_IMPL
        .lock()
        .ok_or(fs_api_v0::FsError::Unsupported)?;
    logging::info!("[fs::rootfs] mount root RW from {}", path);
    // The VFS mutation path uses the RW handle, while ELF loading and other
    // kernel readers use the RO handle.  A RW-only mount leaves the latter
    // unset and makes every user ELF load fail as `RootVolume(Unsupported)`.
    let root_ro = imp.mount_ro(device.clone())?;
    let root = imp.mount_rw(device)?;
    *ROOT_FS.lock() = Some(root_ro);
    *ROOT_RW_FS.lock() = Some(root);
    *ROOT_DEV_PATH.lock() = Some(path.to_string());
    MOUNT_GENERATION.fetch_add(1, Ordering::Release);
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
pub fn mount_generation() -> u64 {
    MOUNT_GENERATION.load(Ordering::Acquire)
}

/// 辅助卷挂载或卸载后递增代次（供 VFS 页缓存失效）。
pub fn bump_mount_generation() {
    MOUNT_GENERATION.fetch_add(1, Ordering::Release);
}

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
