use super::*;
/// devfs 的 [`FsImpl`] 注册项；仅列能力，不参与块卷挂载。
// 本结构代码由AI完成
pub struct KernelDevFsImpl;

/// 全局 devfs impl 实例，供聚合层 `registered_fs_impls()` 引用。
// 本变量代码由AI完成
pub static IMPL: KernelDevFsImpl = KernelDevFsImpl;

#[cfg(feature = "self_test")]
pub fn self_test() {
    logging::info!("[fs/devfs/impl-kernel] self_test begin");
    assert_eq!(linux_vd_disk_path(0), "/dev/vda");
    assert_eq!(linux_vd_disk_path(25), "/dev/vdz");
    logging::info!("[fs/devfs/impl-kernel] self_test complete");
}

// 本变量代码由AI完成
const SUPPORTED: &[FsCapability] =
    &[FsCapability::new(FsKind::DevFs, FsAccessMode::ReadOnly)];

impl FsImpl for KernelDevFsImpl {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn supported(&self) -> &'static [FsCapability] {
        SUPPORTED
    }

// 本方法代码由AI完成
    fn mount_ro(&self, _device: SharedBlockDevice) -> FsResult<SharedFs> {
        Err(FsError::Unsupported)
    }
}
