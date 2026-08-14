use super::*;

pub struct KernelRootFsManager;

// 本变量代码由AI完成
pub(crate) static ROOT_FS: Mutex<Option<fs_api_v0::SharedFs>> = Mutex::new(None);
// 本变量代码由AI完成
pub(crate) static ROOT_RW_FS: Mutex<Option<fs_api_v0::SharedRwFs>> = Mutex::new(None);
// 本变量代码由AI完成
pub(crate) static ROOT_DEV_PATH: Mutex<Option<String>> = Mutex::new(None);
// 生命周期：'static 引用指向注册表中的 impl，由链接期/聚合 init 保证长于内核运行期。
/// 由聚合层在启动期注入的「当前根 FS impl」。聚合层根据 `probe` 选定后调用 [`set_active_fs_impl`]。
// 本变量代码由AI完成
pub(crate) static ACTIVE_FS_IMPL: Mutex<Option<&'static dyn FsImpl>> = Mutex::new(None);

#[cfg(feature = "self_test")]
pub fn self_test() {
    logging::info!("[fs/rootfs/impl-kernel] self_test begin");
    let before = mount_generation();
    assert!(mount_generation() >= before);
    assert_eq!(core::mem::size_of::<KernelRootFsManager>(), 0);
    logging::info!("[fs/rootfs/impl-kernel] self_test complete");
}

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
        bump_mount_generation();
        Ok(())
    }

    fn current_root_device_path(&self) -> Option<String> { ROOT_DEV_PATH.lock().as_ref().cloned() }
}

// 本方法代码由AI完成
