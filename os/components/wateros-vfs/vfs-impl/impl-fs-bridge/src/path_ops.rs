use super::*;

pub fn mount_ext4_block_at(mount_point : &str, block_dev : &str, readonly : bool) -> VfsResult<()> {
    if readonly {
        let aux = fs::mount_aux_ro_from_block_path(block_dev).map_err(map_fs_err)?;
        mount_table::mount_aux_at_ro(mount_point, aux, block_dev)
    } else {
        let aux = fs::mount_aux_rw_from_block_path(block_dev).map_err(map_fs_err)?;
        mount_table::mount_aux_at_rw(mount_point, aux, block_dev)
    }
}

/// 挂载点须为已存在目录（支持 tmpfs 等辅助卷下的路径）。
// 本方法代码由AI完成
#[allow(dead_code)]
pub(crate) fn assert_mount_point_directory(path : &str) -> VfsResult<()> {
    let bridge = FsBridge;
    let meta = bridge.metadata(path)?;
    if meta.node_type != VfsNodeType::Directory {
        return Err(VfsError::NotAFile);
    }
    Ok(())
}

/// 挂载内存 tmpfs 到 `mount_point`。
// 本方法代码由AI完成
pub fn mount_tmpfs_at(mount_point : &str) -> VfsResult<()> {
    mount_table::mount_tmpfs_at(mount_point)
}

/// 挂载带容量上限的 tmpfs 到 `mount_point`。
pub fn mount_tmpfs_at_with_limit(mount_point : &str, limit_bytes : Option<usize>) -> VfsResult<()> {
    mount_table::mount_tmpfs_at_with_limit(mount_point, limit_bytes)
}

/// 在首个任务继承的 bootstrap namespace 中挂载 tmpfs。
pub fn mount_bootstrap_tmpfs_at(mount_point : &str) -> VfsResult<()> {
    mount_table::mount_bootstrap_tmpfs_at(mount_point)
}

/// 挂载 cgroup v1/v2 到 `mount_point`。
// 本方法代码由AI完成
pub fn mount_cgroup_at(mount_point : &str, v2 : bool, options : &str) -> VfsResult<()> {
    mount_table::mount_cgroup_at(mount_point, v2, options)
}

/// 查询路径所在辅助卷的 `statfs` 文件系统 magic。
// 本方法代码由AI完成
pub fn mount_statfs_magic(path : &str) -> Option<isize> { mount_table::mount_statfs_magic(path) }

/// 将 `mount_point` 上已挂载的辅助卷重载为只读。
// 本方法代码由AI完成
pub fn remount_readonly_at(mount_point : &str) -> VfsResult<()> {
    mount_table::remount_aux_readonly(mount_point)
}

/// 卸载 `mount_point` 上的辅助卷。
// 本方法代码由AI完成
pub fn unmount_at(mount_point : &str, detach : bool) -> VfsResult<()> {
    mount_table::unmount_aux_at(mount_point, detach)
}

/// 在 ext4 根卷上创建 `/proc` 挂载点目录（已存在则忽略）。
// 本方法代码由AI完成
pub fn ensure_proc_mount_point() -> VfsResult<()> {
    let path = "/proc";
    let bridge = FsBridge;
    if bridge.exists(path)? {
        return Ok(());
    }
    mkdir_path(path, 0o755)
}

/// 在 ext4 根卷上创建 `/sys` 挂载点目录（已存在则忽略）。
pub fn ensure_sys_mount_point() -> VfsResult<()> {
    let path = "/sys";
    let bridge = FsBridge;
    if bridge.exists(path)? {
        return Ok(());
    }
    mkdir_path(path, 0o755)
}

/// 将 procfs 挂到 `mount_point`（须先 [`ensure_proc_mount_point`]）。
// 本方法代码由AI完成
pub fn mount_procfs_at(mount_point : &str) -> VfsResult<()> {
    mount_table::mount_aux_proc_at(mount_point)
}

/// 挂载 sysfs 到 `mount_point`。
pub fn mount_sysfs_at(mount_point : &str) -> VfsResult<()> {
    mount_table::mount_aux_sys_at(mount_point)
}

// 本方法代码由AI完成
pub fn mount_securityfs_at(mount_point : &str) -> VfsResult<()> {
    mount_table::mount_securityfs_at(mount_point)
}

// 本方法代码由AI完成
pub fn mount_bind_at(source : &str, target : &str, recursive : bool) -> VfsResult<()> {
    mount_table::mount_bind_at(source, target, recursive)
}

// 本方法代码由AI完成
pub fn move_mount_at(source : &str, target : &str) -> VfsResult<()> {
    mount_table::move_mount_at(source, target)
}

pub use mount_table::MountPropagation;

// 本方法代码由AI完成
pub fn set_mount_propagation(mount_point : &str,
                             propagation : MountPropagation,
                             recursive : bool)
                             -> VfsResult<()> {
    mount_table::set_mount_propagation(mount_point, propagation, recursive)
}

pub use mount_table::{
    copy_mount_ns_from_parent, drop_task_mount_ns, init_task_mount_ns, share_mount_ns_from_parent,
    unshare_mount_ns,
};

pub use mount_table::{
    assert_path_writable, is_mount_point, is_proc_mounted_at, list_proc_mount_lines,
    mount_aux_proc_at, mount_aux_sys_at, mount_bootstrap_proc_at, mount_bootstrap_sys_at,
};

/// 删除绝对路径（经挂载表路由）。
// 本方法代码由AI完成
pub fn unlink_path(path : &str, remove_dir : bool) -> VfsResult<()> {
    mount_table::assert_path_writable(path)?;
    let pending_detach = if remove_dir {
        None
    } else {
        paged_handle::prepare_unlink_detach(path)?
    };
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    let result = if remove_dir {
        sess.rmdir(rel.as_str())
    } else {
        sess.unlink(rel.as_str())
    };
    // 仅在删除成功后切断旧路径。打开句柄先切换到 unlink 前的一致快照，
    // 后续同名文件才能使用独立的 path-key cache。
    if result.is_ok() && !remove_dir {
        if let Some(pending) = pending_detach {
            pending.commit();
        }
        let cache = impl_page_cache::global_cache(fs::rootfs::active_impl::mount_generation());
        cache.purge_closed_file(path);
    }
    result
}

/// 将全局文件页缓存和根文件系统缓存同步到底层块设备。
pub fn sync_file_page_cache() -> VfsResult<()> {
    let mount_gen = fs::rootfs::active_impl::mount_generation();
    let cache = impl_page_cache::global_cache(mount_gen);
    let mut io = paged_handle::FsPageIo::path();
    // flush_all 会遍历全局页缓存的所有文件；无磁盘后端（伪文件系统等）的文件
    // 无法写回，flush_key 会失败。Linux syncfs(2) 语义是尽力刷：跳过无法写回
    // 的条目，不阻塞其余文件的写回与根卷 sync。
    match cache.flush_all(&mut io, core::convert::identity) {
        Ok(()) => {}
        Err(VfsError::Unsupported) => {}
        Err(VfsError::ReadOnlyFs) => {}
        Err(VfsError::NotFound) => {}
        Err(e) => return Err(e),
    }
    root_rw()?.lock()
              .sync()
              .map_err(map_fs_err)
}

/// 刷回并丢弃整个文件页缓存（用于测例脚本切换等批量回收点）。
///
/// 先把所有脏页写回根卷，再重建空缓存：既避免长跑后 `files`/LRU 饱和推高内核堆，
/// 也消除历史文件页在饱和时被逐页驱逐、对 ext4 发起越过 EOF 写的隐患。
/// 仅应在已无活跃用户 fd 持有脏页的安全点调用。
// 本方法代码由AI完成
pub fn reset_file_page_cache() -> VfsResult<()> {
    let mount_gen = fs::rootfs::active_impl::mount_generation();
    sync_file_page_cache()?;
    impl_page_cache::reset_global_cache(mount_gen);
    Ok(())
}

/// 创建目录（经挂载表路由）。
// 本方法代码由AI完成
pub fn mkdir_path(path : &str, mode : u32) -> VfsResult<()> {
    let normalized = normalize_absolute_path(path)?;
    if normalized.as_str() == "/" {
        return Err(VfsError::Exists);
    }
    mount_table::assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.mkdir(rel.as_str(), mode)
}

/// 创建符号链接（经挂载表路由）。
// 本方法代码由AI完成
pub fn symlink_path(link_path : &str, target : &str) -> VfsResult<()> {
    let normalized = normalize_absolute_path(link_path)?;
    if normalized.as_str() == "/" {
        return Err(VfsError::InvalidPath);
    }
    mount_table::assert_path_writable(link_path)?;
    let (fs, rel) = fs_and_rel_rw(link_path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.symlink(rel.as_str(), target)
}

/// 创建硬链接（经挂载表路由）。
// 本方法代码由AI完成
pub fn hardlink_path(existing_path : &str, new_path : &str) -> VfsResult<()> {
    let existing = normalize_absolute_path(existing_path)?;
    let new = normalize_absolute_path(new_path)?;
    if new.as_str() == "/" {
        return Err(VfsError::InvalidPath);
    }
    mount_table::assert_path_writable(new.as_str())?;
    let (existing_fs, existing_rel) = fs_and_rel_rw(existing.as_str())?;
    let (new_fs, new_rel) = fs_and_rel_rw(new.as_str())?;
    if !core::ptr::addr_eq(alloc::sync::Arc::as_ptr(&existing_fs),
                           alloc::sync::Arc::as_ptr(&new_fs))
    {
        return Err(VfsError::Unsupported);
    }
    let mut sess = MountedRwSession::new(existing_fs);
    sess.hardlink(existing_rel.as_str(), new_rel.as_str())
}

/// 读取符号链接目标（经挂载表路由）。
// 本方法代码由AI完成
pub fn read_symlink_path(path : &str) -> VfsResult<Vec<u8>> {
    let abs = normalize_absolute_path(path)?;
    if char_dev_exists(abs.as_str()) || special_dev_directory_exists(abs.as_str()) {
        return Err(VfsError::NotAFile);
    }
    match resolve_route(abs.as_str())? {
        FsRoute::PseudoProc { rel, .. } => proc_view().read_symlink(rel.as_str())
                                                      .map_err(map_fs_err),
        FsRoute::PseudoSys { rel, .. } => sys_view().read_symlink(rel.as_str())
                                                    .map_err(map_fs_err),
        FsRoute::PseudoSecurity { .. } => Err(VfsError::NotAFile),
        FsRoute::Root { abs, .. } => root_rw()?.lock()
                                               .read_symlink(abs.as_str())
                                               .map_err(map_fs_err),
        FsRoute::AuxRw { fs, rel, .. } => fs.lock()
                                            .read_symlink(rel.as_str())
                                            .map_err(map_fs_err),
        FsRoute::AuxRo { fs, rel, .. } => fs.lock()
                                            .read_symlink(rel.as_str())
                                            .map_err(map_fs_err),
    }
}

/// 查询路径元数据（经挂载表路由）。
pub fn metadata_path(path : &str) -> VfsResult<VfsMetadata> { FsBridge.metadata(path) }

/// 截断普通文件（经挂载表路由）；同步失效页缓存。
// 本方法代码由AI完成
pub fn truncate_path(path : &str, len : u64) -> VfsResult<()> {
    let normalized = normalize_absolute_path(path)?;
    if char_dev_exists(normalized.as_str()) {
        return Err(VfsError::Unsupported);
    }
    let bridge = FsBridge;
    let meta = bridge.metadata(normalized.as_str())?;
    if meta.node_type != VfsNodeType::File {
        return Err(VfsError::NotAFile);
    }
    mount_table::assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.truncate(rel.as_str(), len)?;
    let mount_gen = fs::rootfs::active_impl::mount_generation();
    impl_page_cache::global_cache(mount_gen).truncate(normalized.as_str(), len);
    Ok(())
}

/// 修改路径权限（经挂载表路由）。
// 本方法代码由AI完成
pub fn chmod_path(path : &str, mode : u32) -> VfsResult<()> {
    let normalized = normalize_absolute_path(path)?;
    if char_dev_exists(normalized.as_str()) {
        return Err(VfsError::Unsupported);
    }
    mount_table::assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.chmod(rel.as_str(), mode)
}

/// 修改路径 uid/gid（经挂载表路由）。
// 本方法代码由AI完成
pub fn chown_path(path : &str, uid : Option<u32>, gid : Option<u32>) -> VfsResult<()> {
    let normalized = normalize_absolute_path(path)?;
    if char_dev_exists(normalized.as_str()) {
        return Err(VfsError::Unsupported);
    }
    if uid.is_none() && gid.is_none() {
        let bridge = FsBridge;
        return bridge.metadata(normalized.as_str())
                     .map(|_| ());
    }
    mount_table::assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.chown(rel.as_str(), uid, gid)
}

// 本变量代码由AI完成
const CGROUP_SUPER_MAGIC : isize = 0x0027_E0EB;
// 本变量代码由AI完成
const CGROUP2_SUPER_MAGIC : isize = 0x6367_7270;

// 本方法代码由AI完成
fn path_on_cgroup_fs(path : &str) -> bool {
    matches!(mount_table::mount_statfs_magic(path),
             Some(CGROUP_SUPER_MAGIC) | Some(CGROUP2_SUPER_MAGIC))
}

/// cgroupfs 扩展属性命名规则（LTP cgroup_xattr）。
// 本方法代码由AI完成
pub fn validate_xattr_name(path : &str, name : &str) -> VfsResult<()> {
    if name.is_empty() || name.len() > 255 {
        return Err(VfsError::InvalidPath);
    }
    if !name.contains('.') {
        return Err(VfsError::InvalidPath);
    }
    if path_on_cgroup_fs(path) {
        if name == "trusted." {
            return Err(VfsError::InvalidPath);
        }
        if name.starts_with("trusted.") && name.len() > "trusted.".len() {
            return Ok(());
        }
        if name.starts_with("security.") {
            return Err(VfsError::Unsupported);
        }
        return Err(VfsError::Unsupported);
    }
    Ok(())
}

// 本方法代码由AI完成
fn xattr_route(path : &str) -> VfsResult<(SharedRwFs, String)> {
    mount_table::assert_path_writable(path)?;
    fs_and_rel_rw(path)
}

// 本方法代码由AI完成
fn map_xattr_get_err(err : VfsError) -> VfsError {
    match err {
        VfsError::InvalidPath => VfsError::NotFound,
        other => other,
    }
}

/// 设置路径扩展属性。
// 本方法代码由AI完成
pub fn setxattr_path(path : &str, name : &str, value : &[u8]) -> VfsResult<()> {
    validate_xattr_name(path, name)?;
    let normalized = normalize_absolute_path(path)?;
    if char_dev_exists(normalized.as_str()) {
        return Err(VfsError::Unsupported);
    }
    let (fs, rel) = xattr_route(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.setxattr(rel.as_str(), name, value)
}

/// 读取路径扩展属性。
// 本方法代码由AI完成
pub fn getxattr_path(path : &str, name : &str, buf : &mut [u8]) -> VfsResult<usize> {
    validate_xattr_name(path, name)?;
    let normalized = normalize_absolute_path(path)?;
    if char_dev_exists(normalized.as_str()) {
        return Err(VfsError::Unsupported);
    }
    let (fs, rel) = fs_and_rel_rw_query(path)?;
    let sess = MountedRwSession::new(fs);
    sess.getxattr(rel.as_str(), name, buf)
        .map_err(map_xattr_get_err)
}

/// 列出路径扩展属性名。
// 本方法代码由AI完成
pub fn listxattr_path(path : &str, buf : &mut [u8]) -> VfsResult<usize> {
    let normalized = normalize_absolute_path(path)?;
    if char_dev_exists(normalized.as_str()) {
        return Err(VfsError::Unsupported);
    }
    let (fs, rel) = fs_and_rel_rw_query(path)?;
    let sess = MountedRwSession::new(fs);
    sess.listxattr(rel.as_str(), buf)
}

/// 删除路径扩展属性。
// 本方法代码由AI完成
pub fn removexattr_path(path : &str, name : &str) -> VfsResult<()> {
    validate_xattr_name(path, name)?;
    let normalized = normalize_absolute_path(path)?;
    if char_dev_exists(normalized.as_str()) {
        return Err(VfsError::Unsupported);
    }
    let (fs, rel) = xattr_route(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.removexattr(rel.as_str(), name)
        .map_err(map_xattr_get_err)
}

// 本方法代码由AI完成
pub(crate) fn replace_file_contents(path : &str, data : &[u8]) -> VfsResult<()> {
    mount_table::assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.write_regular_file(rel.as_str(), data)
}

/// 覆盖绝对路径文件：优先原地 truncate+写入；失败时再 unlink 重建，并驱逐页缓存。
// 本方法代码由AI完成
pub fn overwrite_file_at(path : &str, data : &[u8]) -> VfsResult<()> {
    match replace_file_contents(path, data) {
        Ok(()) => {}
        Err(VfsError::NotFound) => {
            match unlink_path(path, false) {
                Ok(()) => {}
                Err(VfsError::NotFound) => {}
                Err(e) => return Err(e),
            }
            replace_file_contents(path, data)?;
        }
        Err(e) => return Err(e),
    }
    let cache = impl_page_cache::global_cache(fs::rootfs::active_impl::mount_generation());
    cache.purge_closed_file(path);
    Ok(())
}

/// 在绝对路径创建 AF_UNIX 套接字节点（`S_IFSOCK`）。
// 本方法代码由AI完成
pub fn mknod_socket_at(path : &str) -> VfsResult<()> {
    mount_table::assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.mknod(rel.as_str(), 0o140_777, 0)
}

/// 在绝对路径创建普通/特殊节点（`mknodat(2)`）。
// 本方法代码由AI完成
pub fn mknod_path(path : &str, mode : u32, rdev : u32) -> VfsResult<()> {
    mount_table::assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.mknod(rel.as_str(), mode, rdev)
}

/// 重命名绝对路径（经挂载表路由；要求 old/new 落在同一 RW 卷）。
// 本方法代码由AI完成
pub fn rename_path(old_path : &str, new_path : &str) -> VfsResult<()> {
    let old_path = normalize_absolute_path(old_path)?;
    let new_path = normalize_absolute_path(new_path)?;
    if old_path == new_path {
        return Ok(());
    }
    mount_table::assert_path_writable(old_path.as_str())?;
    mount_table::assert_path_writable(new_path.as_str())?;
    let (fs_old, rel_old) = fs_and_rel_rw(old_path.as_str())?;
    let (fs_new, rel_new) = fs_and_rel_rw(new_path.as_str())?;
    if !Arc::ptr_eq(&fs_old, &fs_new) {
        return Err(VfsError::Unsupported);
    }

    // The page cache is path-keyed. Flush before the directory entry moves so
    // a later eviction never tries to write a dirty page through the stale
    // source path and poison an unrelated cache miss.
    let cache = impl_page_cache::global_cache(fs::rootfs::active_impl::mount_generation());
    let mut io = paged_handle::FsPageIo::path();
    cache.flush(&mut io,
                old_path.as_str(),
                core::convert::identity)?;
    cache.flush(&mut io,
                new_path.as_str(),
                core::convert::identity)?;

    let replacement = match FsBridge.metadata(new_path.as_str()) {
        Ok(meta) => Some(meta.node_type),
        Err(VfsError::NotFound) => None,
        Err(e) => return Err(e),
    };
    let pending_detach = if replacement == Some(VfsNodeType::File) {
        paged_handle::prepare_unlink_detach(new_path.as_str())?
    } else {
        None
    };
    let mut sess = MountedRwSession::new(fs_old);
    if let Some(replaced_type) = replacement {
        let temp_path = unused_rename_temp_path(new_path.as_str())?;
        let (temp_fs, rel_temp) = fs_and_rel_rw(temp_path.as_str())?;
        if !Arc::ptr_eq(&sess.inner, &temp_fs) {
            return Err(VfsError::Unsupported);
        }
        sess.rename(rel_new.as_str(), rel_temp.as_str())?;
        if let Err(e) = sess.rename(rel_old.as_str(), rel_new.as_str()) {
            if let Err(rollback) = sess.rename(rel_temp.as_str(), rel_new.as_str()) {
                log::error!("[vfs-rename] target rollback failed temp={} target={} \
                             err={rollback:?}",
                            temp_path,
                            new_path.as_str());
            }
            return Err(e);
        }
        let cleanup = if replaced_type == VfsNodeType::Directory {
            sess.rmdir(rel_temp.as_str())
        } else {
            sess.unlink(rel_temp.as_str())
        };
        if let Err(e) = cleanup {
            log::error!("[vfs-rename] replaced target cleanup failed temp={} err={e:?}",
                        temp_path);
            return Err(e);
        }
    } else {
        sess.rename(rel_old.as_str(), rel_new.as_str())?;
    }
    paged_handle::commit_rename_state(old_path.as_str(),
                                      new_path.as_str(),
                                      pending_detach);
    Ok(())
}

fn unused_rename_temp_path(target : &str) -> VfsResult<String> {
    let parent = target.rsplit_once('/')
                       .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
                       .unwrap_or("/");
    for _ in 0..64 {
        let id = RENAME_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = if parent == "/" {
            format!("/.wateros-rename-replaced-{id}")
        } else {
            format!("{parent}/.wateros-rename-replaced-{id}")
        };
        match FsBridge.metadata(candidate.as_str()) {
            Err(VfsError::NotFound) => return Ok(candidate),
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }
    Err(VfsError::Exists)
}

/// 与只读根句柄分离的可写挂载会话。
// 本结构代码由AI完成
pub struct MountedRwSession {
    pub(crate) inner : SharedRwFs,
}
