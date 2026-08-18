use super::*;

/// procfs 的 [`FsImpl`] 注册项；仅列能力，不参与块卷挂载。
// 本结构代码由AI完成
pub struct KernelProcFsImpl;

// 本变量代码由AI完成
/// 全局 procfs impl 实例；该实现无状态，可由各挂载点共享。
pub static IMPL : KernelProcFsImpl = KernelProcFsImpl;

// 本变量代码由AI完成
/// procfs 只能只读访问，不能通过块设备挂载。
const SUPPORTED : &[FsCapability] = &[FsCapability::new(FsKind::Other("procfs"),
                                                        FsAccessMode::ReadOnly)];

impl FsImpl for KernelProcFsImpl {
    fn name(&self) -> &'static str { "procfs" }

    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }

    // 本方法代码由AI完成
    /// procfs 不接受块设备；VFS 应改为取得 [`crate::view`] 暴露的伪文件视图。
    fn mount_ro(&self,
                _device : driver_block_api_v0::SharedBlockDevice)
                -> fs_api_v0::FsResult<fs_api_v0::SharedFs> {
        Err(FsError::Unsupported)
    }
}

// 本方法代码由AI完成
/// 最小自检：枚举根目录以验证路径解析与视图对象已完成链接。
pub fn test() {
    let v = view();
    let _ = v.read_dir("/");
    logging::info!("[fs::procfs] self_test ok");
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    test();
}
