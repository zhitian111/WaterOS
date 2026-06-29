//! ext4 自检：RO 与 RW 根卷路径。
//! 本模块代码由AI完成

use alloc::{string::String, vec};
use api_v0::{FsNodeType, FsResult, ReadOnlyFs, ReadWriteFs, SharedFs, SharedRwFs};
use wateros_base_config::fs::FILE_PAGE_SIZE;

/// 用已挂载的只读 ext4 句柄做最小 RO 自检（不依赖镜像内固定示例文件路径）。
// 本方法代码由AI完成
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
    ro_range_smoke_readonly(&*fs)?;
    Ok(())
}

/// 用已挂载的 RW ext4 句柄做根目录与 `read_range` 烟囱（bring-up 主路径）。
// 本方法代码由AI完成
pub fn rw_self_test(rw: SharedRwFs) -> FsResult<()> {
    let fs = rw.lock();
    match fs.metadata("/") {
        Ok(meta) => {
            logging::info!(
                "[fs::ext4][test] RW root metadata OK type={:?} size={} mode={:#o}",
                meta.node_type,
                meta.size,
                meta.mode
            );
        }
        Err(err) => {
            logging::warn!("[fs::ext4][test] RW root metadata FAIL err={:?}", err);
            return Err(err);
        }
    }
    rw_range_smoke(&*fs)?;
    Ok(())
}

/// 对镜像内首个足够大的普通文件做 `read_range` 前缀读，验证不依赖整文件 `read`。
// 本方法代码由AI完成
fn ro_range_smoke_readonly(fs: &dyn ReadOnlyFs) -> FsResult<()> {
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

// 本方法代码由AI完成
fn rw_range_smoke(fs: &dyn ReadWriteFs) -> FsResult<()> {
    let candidates = [
        "/elf/000_hello_world.elf",
        "/src/bin/000_hello_world.rs",
        "/glibc/basic/text.txt",
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
            "[fs::ext4][test] RW read_range prefix OK path={} n={}",
            path,
            n
        );
        assert!(n > 0 && n <= FILE_PAGE_SIZE);
        return Ok(());
    }
    logging::info!("[fs::ext4][test] RW read_range smoke skipped (no large candidate)");
    Ok(())
}

/// RW：在根下创建临时目录并用同一 RW 句柄校验类型。
// 本方法代码由AI完成
pub fn rw_mkdir_verify(rw: SharedRwFs, dir_name: &str) -> FsResult<()> {
    if dir_name.is_empty() || dir_name.contains('/') {
        return Err(api_v0::FsError::InvalidPath);
    }
    let mut path = String::from("/");
    path.push_str(dir_name);
    {
        let mut guard = rw.lock();
        guard.mkdir(path.as_str(), 0o755)?;
    }
    let meta = rw.lock().metadata(path.as_str())?;
    if meta.node_type != FsNodeType::Directory {
        return Err(api_v0::FsError::Io);
    }
    logging::info!("[fs::ext4][test] mkdir OK path={}", path);
    Ok(())
}
