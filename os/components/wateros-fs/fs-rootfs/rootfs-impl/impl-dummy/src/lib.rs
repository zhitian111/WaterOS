#![no_std]
extern crate alloc;

pub struct DummyRootFsManager;

impl api_v0::RootFsManager for DummyRootFsManager {
    fn set_root_fs(&mut self, _fs: fs_api_v0::SharedFs) {}

    fn root_fs(&self) -> Option<fs_api_v0::SharedFs> { None }

    fn clear_root_fs(&mut self) {}

    fn mount_root_from_block_path(&mut self, _path: &str) -> fs_api_v0::FsResult<()> {
        Err(fs_api_v0::FsError::Unsupported)
    }

    fn current_root_device_path(&self) -> Option<alloc::string::String> { None }
}
