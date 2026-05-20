//! ext4 自检：RO 端校验根目录可读（与具体镜像内容解耦）。

use alloc::vec;
use api_v0::{FsNodeType, FsResult, ReadOnlyFs, SharedFs};
use wateros_base_config::fs::FILE_PAGE_SIZE;

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
    ro_range_smoke(&*fs)?;
    Ok(())
}

/// 对镜像内首个足够大的普通文件做 `read_range` 前缀读，验证不依赖整文件 `read`。
fn ro_range_smoke(fs: &dyn ReadOnlyFs) -> FsResult<()> {
    let candidates = [
        "/elf/000_hello_world.elf",
        "/src/bin/000_hello_world.rs",
    ];
    for path in candidates {
        let Ok(meta) = fs.metadata(path) else {
            continue;
        };
        if meta.node_type != FsNodeType::File {
            continue;
        }
        if meta.size < FILE_PAGE_SIZE as u64 {
            continue;
        }
        let mut buf = vec![0u8; FILE_PAGE_SIZE];
        let n = fs.read_range(path, 0, &mut buf)?;
        logging::info!(
            "[fs::ext4][test] read_range prefix OK path={} n={}",
            path,
            n
        );
        assert!(n > 0 && n <= FILE_PAGE_SIZE);
        return Ok(());
    }
    logging::info!("[fs::ext4][test] read_range smoke skipped (no large candidate)");
    Ok(())
}
