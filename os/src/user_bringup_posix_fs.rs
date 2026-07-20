//! `[bringup][posix-fs-meta]`：目录创建、枚举、重命名与删除烟囱（不依赖第二块盘）。

use runtime::logging::*;

/// 在根卷上执行 mkdir → read_dir → rename → rmdir 烟囱。
#[allow(unused)]
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
        const DIR_RENAMED : &str = "/__posix_fs_meta_dir_renamed";
        const FILE : &str = "/__posix_fs_meta_dir/entry.txt";
        const DATA : &[u8] = b"posix-fs-meta";

        match run_smoke(DIR, DIR_RENAMED, FILE, DATA) {
            Ok(count) => {
                info!("[posix-fs-meta] PASS entry_count={count} path={DIR_RENAMED}");
            }
            Err(e) => {
                warn!("[posix-fs-meta] FAIL: {e:?}");
            }
        }
    }
    info!("[bringup][posix-fs-meta] END");
}
#[allow(unused)]
#[cfg(feature = "vfs-bridge")]
fn run_smoke(dir : &str,
             dir_renamed : &str,
             file : &str,
             data : &[u8])
             -> Result<usize, vfs::api::VfsError> {
    use vfs::api::SingleRootReadView;

    let mut sess = vfs::mount::open_rw_session(vfs::api::VfsFsKind::Ext4)?;
    sess.mkdir(dir, 0o755)?;
    sess.write_regular_file(file, data)?;

    let view = vfs::root::read_view();
    let entries = view.read_dir(dir)?;
    let count = entries.len();

    sess.unlink(file)?;
    sess.rename(dir, dir_renamed)?;

    if view.exists(dir)? {
        return Err(vfs::api::VfsError::Io);
    }
    if !view.exists(dir_renamed)? {
        return Err(vfs::api::VfsError::Io);
    }

    sess.rmdir(dir_renamed)?;

    Ok(count)
}
