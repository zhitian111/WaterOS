#![no_std]
extern crate alloc;

use alloc::string::{String, ToString};
use api_v0::RootFsManager;
use spin::Mutex;

pub struct KernelRootFsManager;

static ROOT_FS: Mutex<Option<fs_api_v0::SharedFs>> = Mutex::new(None);
static ROOT_DEV_PATH: Mutex<Option<String>> = Mutex::new(None);

impl api_v0::RootFsManager for KernelRootFsManager {
    fn set_root_fs(&mut self, fs: fs_api_v0::SharedFs) {
        *ROOT_FS.lock() = Some(fs);
    }

    fn root_fs(&self) -> Option<fs_api_v0::SharedFs> { ROOT_FS.lock().as_ref().cloned() }

    fn clear_root_fs(&mut self) {
        *ROOT_FS.lock() = None;
        *ROOT_DEV_PATH.lock() = None;
    }

    fn mount_root_from_block_path(&mut self, path: &str) -> fs_api_v0::FsResult<()> {
        let root = impl_ext4_view::mount_by_block_path(path)?;
        self.set_root_fs(root);
        *ROOT_DEV_PATH.lock() = Some(path.to_string());
        Ok(())
    }

    fn current_root_device_path(&self) -> Option<String> { ROOT_DEV_PATH.lock().as_ref().cloned() }
}

pub fn mount_default_root() -> fs_api_v0::FsResult<()> {
    let Some(path) = devfs::active_impl::default_root_block_path() else {
        return Err(fs_api_v0::FsError::NotMounted);
    };
    logging::info!("[fs::rootfs] mount default root from {}", path);
    let mut mgr = KernelRootFsManager;
    mgr.mount_root_from_block_path(path.as_str())
}

pub fn root_fs() -> Option<fs_api_v0::SharedFs> {
    let mgr = KernelRootFsManager;
    mgr.root_fs()
}
