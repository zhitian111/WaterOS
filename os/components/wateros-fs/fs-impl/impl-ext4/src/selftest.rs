//! ext4 自检：RO 端校验根目录可读（与具体镜像内容解耦）。

use api_v0::{FsResult, ReadOnlyFs, SharedFs};

/// 用已挂载的只读 ext4 句柄做最小 RO 自检（不依赖镜像内固定示例文件路径）。
pub fn ro_self_test(fs: SharedFs) -> FsResult<()> {
    let fs = fs.lock();
    match fs.metadata("/") {
        Ok(meta) => {
            logging::info!(
                "[fs::ext4][test] root metadata OK type={:?} size={} mode={:#o}",
                meta.node_type,
                meta.size,
                meta.mode
            );
        }
        Err(err) => {
            logging::warn!("[fs::ext4][test] root metadata FAIL err={:?}", err);
            return Err(err);
        }
    }
    Ok(())
}
