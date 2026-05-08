#![no_std]

//! 内核 rootfs 实现：静态槽位保存当前根 [`SharedFs`]、根块路径与聚合层注入的 [`FsImpl`]。
//!
//! 挂载前须由外层调用 [`set_active_fs_impl`]；否则 [`mount_default_root`] 与 [`RootFsManager::mount_root_from_block_path`] 将失败。
extern crate alloc;

use alloc::string::{String, ToString};
use api_v0::RootFsManager;
use fs_api_v0::FsImpl;
use spin::Mutex;

/// 零大小 [`RootFsManager`] 句柄；读写 `static ROOT_FS` 等状态。
pub struct KernelRootFsManager;

static ROOT_FS: Mutex<Option<fs_api_v0::SharedFs>> = Mutex::new(None);
static ROOT_DEV_PATH: Mutex<Option<String>> = Mutex::new(None);
// 生命周期：'static 引用指向注册表中的 impl，由链接期/聚合 init 保证长于内核运行期。
/// 由聚合层在启动期注入的「当前根 FS impl」。聚合层根据 `probe` 选定后调用 [`set_active_fs_impl`]。
static ACTIVE_FS_IMPL: Mutex<Option<&'static dyn FsImpl>> = Mutex::new(None);

impl api_v0::RootFsManager for KernelRootFsManager {
    fn set_root_fs(&mut self, fs: fs_api_v0::SharedFs) {
        *ROOT_FS.lock() = Some(fs);
    }

    // 克隆 Arc：调用方获得独立句柄，与槽位内实例共享底层卷状态。
    fn root_fs(&self) -> Option<fs_api_v0::SharedFs> { ROOT_FS.lock().as_ref().cloned() }

    fn clear_root_fs(&mut self) {
        *ROOT_FS.lock() = None;
        *ROOT_DEV_PATH.lock() = None;
    }

    fn mount_root_from_block_path(&mut self, path: &str) -> fs_api_v0::FsResult<()> {
        // devfs 解析路径 → 活动 FsImpl RO 挂载 → 记录路径，三步任一失败即向上返回。
        let device = devfs::active_impl::lookup_block_device(path)?;
        let imp = ACTIVE_FS_IMPL
            .lock()
            .ok_or(fs_api_v0::FsError::Unsupported)?;
        let root = imp.mount_ro(device)?;
        self.set_root_fs(root);
        *ROOT_DEV_PATH.lock() = Some(path.to_string());
        Ok(())
    }

    fn current_root_device_path(&self) -> Option<String> { ROOT_DEV_PATH.lock().as_ref().cloned() }
}

/// 由聚合层注入选好的 FS impl（按 `FsImpl::probe` 与 `supports(kind, ReadOnly)` 选取）。
pub fn set_active_fs_impl(imp: &'static dyn FsImpl) {
    *ACTIVE_FS_IMPL.lock() = Some(imp);
}

/// 返回当前注入的活动 [`FsImpl`]；未注入时为 `None`。
pub fn active_fs_impl() -> Option<&'static dyn FsImpl> { *ACTIVE_FS_IMPL.lock() }

/// 查询 `devfs` 活动实现的默认根块路径并调用 [`RootFsManager::mount_root_from_block_path`]。
pub fn mount_default_root() -> fs_api_v0::FsResult<()> {
    let Some(path) = devfs::active_impl::default_root_block_path() else {
        return Err(fs_api_v0::FsError::NotMounted);
    };
    logging::info!("[fs::rootfs] mount default root from {}", path);
    let mut mgr = KernelRootFsManager;
    mgr.mount_root_from_block_path(path.as_str())
}

/// 当前根只读文件系统句柄；未挂载返回 `None`。
pub fn root_fs() -> Option<fs_api_v0::SharedFs> {
    let mgr = KernelRootFsManager;
    mgr.root_fs()
}

/// 最近一次成功挂载根卷所使用的块设备路径。
pub fn current_root_device_path() -> Option<String> {
    let mgr = KernelRootFsManager;
    mgr.current_root_device_path()
}
