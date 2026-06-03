//! `[bringup][posix-fs-meta]`：目录创建、枚举与删除烟囱（不依赖第二块盘）。

use runtime::logging::*;

/// 在根卷上执行 mkdir → read_dir → unlink 烟囱。
pub fn run_stage_posix_fs_meta() {
    info!("[bringup][posix-fs-meta] BEGIN");
    #[cfg(not(feature = "vfs-bridge"))]
    {
        warn!("[posix-fs-meta] vfs-bridge off: skip");
        info!("[bringup][posix-fs-meta] END");
        return;
    }
    #[cfg(feature = "vfs-bridge")]
    {
        const DIR : &str = "/__posix_fs_meta_dir";
        const FILE : &str = "/__posix_fs_meta_dir/entry.txt";
        const DATA : &[u8] = b"posix-fs-meta";

        match run_smoke(DIR, FILE, DATA) {
            Ok(count) => {
                info!("[posix-fs-meta] PASS entry_count={count} path={DIR}");
            }
            Err(e) => {
                warn!("[posix-fs-meta] FAIL: {e:?}");
            }
        }
    }
    info!("[bringup][posix-fs-meta] END");
}

#[cfg(feature = "vfs-bridge")]
fn run_smoke(dir : &str, file : &str, data : &[u8]) -> Result<usize, vfs::api::VfsError> {
    use vfs::api::SingleRootReadView;

    let mut sess = vfs::mount::open_rw_session(vfs::api::VfsFsKind::Ext4)?;
    sess.mkdir(dir, 0o755)?;
    sess.write_regular_file(file, data)?;

    let view = vfs::root::read_view();
    let entries = view.read_dir(dir)?;
    let count = entries.len();

    sess.unlink(file)?;
    sess.rmdir(dir)?;

    Ok(count)
}
