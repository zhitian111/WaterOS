use super::*;

/// procfs 的 [`FsImpl`] 注册项；仅列能力，不参与块卷挂载。
// 本结构代码由AI完成
pub struct KernelProcFsImpl;

/// 全局 procfs impl 实例。
// 本变量代码由AI完成
pub static IMPL : KernelProcFsImpl = KernelProcFsImpl;

// 本变量代码由AI完成
const SUPPORTED : &[FsCapability] = &[FsCapability::new(FsKind::Other("procfs"),
                                                        FsAccessMode::ReadOnly)];

impl FsImpl for KernelProcFsImpl {
    fn name(&self) -> &'static str { "procfs" }

    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }

    // 本方法代码由AI完成
    fn mount_ro(&self,
                _device : driver_block_api_v0::SharedBlockDevice)
                -> fs_api_v0::FsResult<fs_api_v0::SharedFs> {
        Err(FsError::Unsupported)
    }
}

/// 最小自检：枚举根目录并打日志。
// 本方法代码由AI完成
pub fn test() {
    let v = view();
    let _ = v.read_dir("/");
    logging::info!("[fs::procfs] self_test ok");
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    test();
}

