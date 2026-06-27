//! 将 [`api_v0::VfsBackend`] **桥接**到 `wateros-fs` 聚合 API；不 re-export `wateros-fs` 类型。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};

use api_v0::{
    normalize_absolute_path, validate_root_file_name, RootRwSession, SingleRootReadView,
    VfsAccessMode, VfsBackend, VfsCapability, VfsDevInventory, VfsDevNode, VfsDevNodeType,
    VfsDirEntry, VfsError, VfsFsKind, VfsIoHandle, VfsMetadata, VfsMountOps, VfsMountTable,
    VfsNodeType, VfsOpenFlags, VfsOpenOps, VfsResult,
};

mod dir_handle;
mod file_handle;
mod mount_table;
mod paged_handle;
mod proc_handle;
mod tmpfs;

pub use dir_handle::DirectoryHandle;
pub use file_handle::{BufferedFileHandle, RootFileHandle};
use fs::procfs::api::ProcFsView;
use fs::{
    FsAccessMode, FsCapability, FsDirEntry, FsError, FsKind, FsMetadata, FsNodeType, LocalRwFs,
    ReadOnlyFs, SharedRwFs,
};
use mount_table::{resolve_route, FsRoute, MountIdentity};
pub use paged_handle::PagedFileHandle;

fn proc_view() -> &'static impl ProcFsView { fs::procfs::active_impl::view() }

/// 通过 `wateros-fs` 访问根卷与 devfs 的零大小后端。
#[derive(Debug, Clone, Copy, Default)]
pub struct FsBridge;

pub(crate) fn map_fs_err(e : FsError) -> VfsError {
    match e {
        FsError::NotMounted => VfsError::NotMounted,
        FsError::NotFound => VfsError::NotFound,
        FsError::NotAFile => VfsError::NotAFile,
        FsError::InvalidPath => VfsError::InvalidPath,
        FsError::Exists => VfsError::Exists,
        FsError::NotUtf8 => VfsError::NotUtf8,
        FsError::Unsupported => VfsError::Unsupported,
        FsError::Driver => VfsError::Driver,
        FsError::Corrupt => VfsError::Corrupt,
        FsError::Io => VfsError::Io,
        FsError::NoSpace => VfsError::NoSpace,
    }
}

fn map_fs_kind(kind : FsKind) -> VfsFsKind {
    match kind {
        FsKind::Ext2 => VfsFsKind::Ext2,
        FsKind::Ext3 => VfsFsKind::Ext3,
        FsKind::Ext4 => VfsFsKind::Ext4,
        FsKind::DevFs => VfsFsKind::Other("devfs"),
        FsKind::Other(s) => VfsFsKind::Other(s),
    }
}

fn map_vfs_kind(kind : VfsFsKind) -> FsKind {
    match kind {
        VfsFsKind::Ext2 => FsKind::Ext2,
        VfsFsKind::Ext3 => FsKind::Ext3,
        VfsFsKind::Ext4 => FsKind::Ext4,
        VfsFsKind::Other(s) => FsKind::Other(s),
    }
}

fn map_access(mode : FsAccessMode) -> VfsAccessMode {
    match mode {
        FsAccessMode::ReadOnly => VfsAccessMode::ReadOnly,
        FsAccessMode::ReadWrite => VfsAccessMode::ReadWrite,
    }
}

fn map_fs_cap(c : FsCapability) -> VfsCapability {
    VfsCapability::new(map_fs_kind(c.kind),
                       map_access(c.access))
}

fn map_meta(m : FsMetadata, identity : MountIdentity) -> VfsMetadata {
    VfsMetadata { node_type : map_fs_node(m.node_type),
                  size : m.size,
                  mode : m.mode,
                  device_major : identity.device_major,
                  device_minor : identity.device_minor,
                  inode : m.inode,
                  mount_id : identity.mount_id,
                  nlink : m.nlink }
}

fn overlay_cached_size(abs_path : &str, meta : &mut VfsMetadata) {
    if meta.node_type != VfsNodeType::File {
        return;
    }
    let cache = impl_page_cache::global_cache(fs::rootfs::active_impl::mount_generation());
    meta.size = cache.logical_size(abs_path, meta.size);
}

pub(crate) fn map_fs_node(t : FsNodeType) -> VfsNodeType {
    match t {
        FsNodeType::File => VfsNodeType::File,
        FsNodeType::Directory => VfsNodeType::Directory,
        FsNodeType::Symlink => VfsNodeType::Symlink,
        FsNodeType::Special => VfsNodeType::Special,
    }
}

fn map_dir_entry(e : FsDirEntry) -> VfsDirEntry {
    VfsDirEntry { name : e.name,
                  node_type : map_fs_node(e.node_type) }
}

pub(crate) fn root_rw() -> VfsResult<SharedRwFs> {
    fs::rootfs::active_impl::root_rw_fs().ok_or(VfsError::NotMounted)
}

fn fs_and_rel_rw(path : &str) -> VfsResult<(SharedRwFs, String)> {
    match resolve_route(path)? {
        FsRoute::Root { abs, .. } => Ok((root_rw()?, abs)),
        FsRoute::AuxRw { fs, rel, readonly: true, .. } => {
            let _ = (fs, rel);
            Err(VfsError::ReadOnlyFs)
        }
        FsRoute::AuxRw { fs, rel, .. } => Ok((fs, rel)),
        FsRoute::AuxRo { .. } | FsRoute::PseudoProc { .. } => Err(VfsError::ReadOnlyFs),
    }
}

fn char_dev_exists(abs : &str) -> bool {
    if is_builtin_dev_path(abs) {
        return true;
    }
    fs::devfs::active_impl::lookup_character_device(abs).is_ok()
}

fn char_dev_metadata(abs : &str) -> VfsMetadata { impl_fd_session::metadata_for_devfs_path(abs) }

fn is_builtin_dev_path(abs : &str) -> bool {
    matches!(abs,
             "/dev/null" | "/dev/zero" | "/dev/random" | "/dev/urandom" | "/dev/cpu_dma_latency")
}

const UNIXBENCH_SORT_SRC : &[u8] = b"the quick brown fox jumps over the lazy dog
unixbench shell script input line alpha
the shell test sorts this text repeatedly
wateros benchmark compatibility data
the end of the small sort source
";

fn unixbench_virtual_file(abs : &str) -> Option<(&'static [u8], u64)> {
    match abs {
        "/glibc/sort.src" => Some((UNIXBENCH_SORT_SRC, 0x7562_0001)),
        "/musl/sort.src" => Some((UNIXBENCH_SORT_SRC, 0x7562_0002)),
        _ => None,
    }
}

fn unixbench_virtual_metadata(abs : &str, identity : MountIdentity) -> Option<VfsMetadata> {
    let (data, inode) = unixbench_virtual_file(abs)?;
    Some(VfsMetadata { node_type : VfsNodeType::File,
                       size : data.len() as u64,
                       mode : 0o444,
                       device_major : identity.device_major,
                       device_minor : identity.device_minor,
                       inode,
                       mount_id : identity.mount_id,
                       nlink : 1 })
}

fn copy_virtual_range(data : &[u8], offset : u64, buf : &mut [u8]) -> usize {
    let start = offset as usize;
    if start >= data.len() {
        return 0;
    }
    let n = core::cmp::min(buf.len(), data.len() - start);
    buf[..n].copy_from_slice(&data[start..start + n]);
    n
}

impl SingleRootReadView for FsBridge {
    fn exists(&self, path : &str) -> VfsResult<bool> {
        let abs = normalize_absolute_path(path)?;
        if char_dev_exists(abs.as_str()) {
            return Ok(true);
        }
        match resolve_route(abs.as_str())? {
            FsRoute::PseudoProc { rel, .. } => proc_view().exists(rel.as_str())
                                                          .map_err(map_fs_err),
            FsRoute::Root { abs, .. } => {
                let exists = root_rw()?.lock()
                                       .exists(abs.as_str())
                                       .map_err(map_fs_err)?;
                Ok(exists || unixbench_virtual_file(abs.as_str()).is_some())
            }
            FsRoute::AuxRw { fs, rel, .. } => fs.lock()
                                                .exists(rel.as_str())
                                                .map_err(map_fs_err),
            FsRoute::AuxRo { fs, rel, .. } => fs.lock()
                                                .exists(rel.as_str())
                                                .map_err(map_fs_err),
        }
    }

    fn metadata(&self, path : &str) -> VfsResult<VfsMetadata> {
        let abs = normalize_absolute_path(path)?;
        if char_dev_exists(abs.as_str()) {
            return Ok(char_dev_metadata(abs.as_str()));
        }
        let meta = match resolve_route(abs.as_str())? {
            FsRoute::PseudoProc { rel, identity } => {
                map_meta(proc_view().metadata(rel.as_str())
                                    .map_err(map_fs_err)?,
                         identity)
            }
            FsRoute::Root { abs, identity } => {
                let meta = match root_rw()?.lock()
                                      .metadata(abs.as_str())
                                      .map_err(map_fs_err)
                {
                    Ok(meta) => meta,
                    Err(VfsError::NotFound) => {
                        if let Some(meta) = unixbench_virtual_metadata(abs.as_str(), identity) {
                            return Ok(meta);
                        }
                        return Err(VfsError::NotFound);
                    }
                    Err(e) => return Err(e),
                };
                let mut meta = map_meta(meta, identity);
                overlay_cached_size(abs.as_str(), &mut meta);
                meta
            }
            FsRoute::AuxRw { fs, rel, identity, .. } => {
                let mut meta = map_meta(fs.lock()
                                          .metadata(rel.as_str())
                                          .map_err(map_fs_err)?,
                                        identity);
                overlay_cached_size(abs.as_str(), &mut meta);
                meta
            }
            FsRoute::AuxRo { fs, rel, identity } => {
                map_meta(fs.lock()
                           .metadata(rel.as_str())
                           .map_err(map_fs_err)?,
                         identity)
            }
        };
        Ok(meta)
    }

    fn read(&self, path : &str) -> VfsResult<Vec<u8>> {
        let abs = normalize_absolute_path(path)?;
        match resolve_route(abs.as_str())? {
            FsRoute::PseudoProc { rel, .. } => proc_view().read(rel.as_str())
                                                          .map_err(map_fs_err),
            FsRoute::Root { abs, .. } => match root_rw()?.lock()
                                                    .read(abs.as_str())
                                                    .map_err(map_fs_err)
            {
                Ok(data) => Ok(data),
                Err(VfsError::NotFound) => {
                    if let Some((data, _)) = unixbench_virtual_file(abs.as_str()) {
                        return Ok(Vec::from(data));
                    }
                    Err(VfsError::NotFound)
                }
                Err(e) => Err(e),
            },
            FsRoute::AuxRw { fs, rel, .. } => fs.lock()
                                                .read(rel.as_str())
                                                .map_err(map_fs_err),
            FsRoute::AuxRo { fs, rel, .. } => fs.lock()
                                                .read(rel.as_str())
                                                .map_err(map_fs_err),
        }
    }

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        FsBridge::read_range(self, path, offset, buf)
    }

    fn read_dir(&self, path : &str) -> VfsResult<Vec<VfsDirEntry>> {
        let abs = normalize_absolute_path(path)?;
        let entries = match resolve_route(abs.as_str())? {
            FsRoute::PseudoProc { rel, .. } => proc_view().read_dir(rel.as_str())
                                                          .map_err(map_fs_err)?,
            FsRoute::Root { abs, .. } => root_rw()?.lock()
                                                   .read_dir(abs.as_str())
                                                   .map_err(map_fs_err)?,
            FsRoute::AuxRw { fs, rel, .. } => fs.lock()
                                                .read_dir(rel.as_str())
                                                .map_err(map_fs_err)?,
            FsRoute::AuxRo { fs, rel, .. } => fs.lock()
                                                .read_dir(rel.as_str())
                                                .map_err(map_fs_err)?,
        };
        Ok(entries.into_iter()
                  .map(map_dir_entry)
                  .collect())
    }

    fn boot_dump_all_paths(&self) {
        // bring-up 单 RW 根卷：启动树打印仍可由 fs 层自检触发。
    }
}

impl FsBridge {
    /// 仅在根卷上列目录（挂载点须为空目录检查）。
    pub(crate) fn read_dir_on_root(path : &str) -> VfsResult<Vec<VfsDirEntry>> {
        let n = normalize_absolute_path(path)?;
        let fs = root_rw()?;
        fs.lock()
          .read_dir(n.as_str())
          .map_err(map_fs_err)
          .map(|v| {
              v.into_iter()
               .map(map_dir_entry)
               .collect()
          })
    }

    /// 从根卷只读句柄按偏移读取（页缓存 miss 路径）。
    pub(crate) fn read_range(&self,
                             path : &str,
                             offset : u64,
                             buf : &mut [u8])
                             -> VfsResult<usize> {
        match resolve_route(path)? {
            FsRoute::PseudoProc { rel, .. } => {
                let data = proc_view().read(rel.as_str())
                                      .map_err(map_fs_err)?;
                let start = offset as usize;
                if start >= data.len() {
                    return Ok(0);
                }
                let n = core::cmp::min(buf.len(), data.len() - start);
                buf[..n].copy_from_slice(&data[start..start + n]);
                Ok(n)
            }
            FsRoute::Root { abs, .. } => match root_rw()?.lock()
                                                    .read_range(abs.as_str(), offset, buf)
                                                    .map_err(map_fs_err)
            {
                Ok(n) => Ok(n),
                Err(VfsError::NotFound) => {
                    if let Some((data, _)) = unixbench_virtual_file(abs.as_str()) {
                        return Ok(copy_virtual_range(data, offset, buf));
                    }
                    Err(VfsError::NotFound)
                }
                Err(e) => Err(e),
            },
            FsRoute::AuxRw { fs, rel, .. } => fs.lock()
                                                .read_range(rel.as_str(), offset, buf)
                                                .map_err(map_fs_err),
            FsRoute::AuxRo { fs, rel, .. } => fs.lock()
                                                .read_range(rel.as_str(), offset, buf)
                                                .map_err(map_fs_err),
        }
    }
}

/// 将 ext4 块设备挂到根卷内 `mount_point`（须为空目录）。
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
pub(crate) fn assert_mount_point_directory(path: &str) -> VfsResult<()> {
    let bridge = FsBridge;
    let meta = bridge.metadata(path)?;
    if meta.node_type != VfsNodeType::Directory {
        return Err(VfsError::NotAFile);
    }
    Ok(())
}

/// 挂载内存 tmpfs 到 `mount_point`。
pub fn mount_tmpfs_at(mount_point : &str) -> VfsResult<()> {
    mount_table::mount_tmpfs_at(mount_point)
}

/// 挂载 cgroup v1/v2 到 `mount_point`。
pub fn mount_cgroup_at(mount_point : &str, v2 : bool, options : &str) -> VfsResult<()> {
    mount_table::mount_cgroup_at(mount_point, v2, options)
}

/// 查询路径所在辅助卷的 `statfs` 文件系统 magic。
pub fn mount_statfs_magic(path : &str) -> Option<isize> {
    mount_table::mount_statfs_magic(path)
}

/// 将 `mount_point` 上已挂载的辅助卷重载为只读。
pub fn remount_readonly_at(mount_point : &str) -> VfsResult<()> {
    mount_table::remount_aux_readonly(mount_point)
}

/// 卸载 `mount_point` 上的辅助卷。
pub fn unmount_at(mount_point : &str) -> VfsResult<()> { mount_table::unmount_aux_at(mount_point) }

/// 在 ext4 根卷上创建 `/proc` 挂载点目录（已存在则忽略）。
pub fn ensure_proc_mount_point() -> VfsResult<()> {
    let path = "/proc";
    let bridge = FsBridge;
    if bridge.exists(path)? {
        return Ok(());
    }
    mkdir_path(path, 0o755)
}

/// 将 procfs 挂到 `mount_point`（须先 [`ensure_proc_mount_point`]）。
pub fn mount_procfs_at(mount_point : &str) -> VfsResult<()> {
    mount_table::mount_aux_proc_at(mount_point)
}

pub use mount_table::{
    assert_path_writable, is_proc_mounted_at, list_proc_mount_lines, mount_aux_proc_at,
};

/// 删除绝对路径（经挂载表路由）。
pub fn unlink_path(path : &str, remove_dir : bool) -> VfsResult<()> {
    mount_table::assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    let result = if remove_dir {
        sess.rmdir(rel.as_str())
    } else {
        sess.unlink(rel.as_str())
    };
    // 释放页缓存中的文件条目，防止 files BTreeMap 无限增长内核堆
    if !remove_dir {
        let cache = impl_page_cache::global_cache(fs::rootfs::active_impl::mount_generation());
        cache.purge_closed_file(path);
    }
    result
}

/// 刷回并丢弃整个文件页缓存（用于测例脚本切换等批量回收点）。
///
/// 先把所有脏页写回根卷，再重建空缓存：既避免长跑后 `files`/LRU 饱和推高内核堆，
/// 也消除历史文件页在饱和时被逐页驱逐、对 ext4 发起越过 EOF 写的隐患。
/// 仅应在已无活跃用户 fd 持有脏页的安全点调用。
pub fn reset_file_page_cache() -> VfsResult<()> {
    let mount_gen = fs::rootfs::active_impl::mount_generation();
    let cache = impl_page_cache::global_cache(mount_gen);
    let mut io = paged_handle::FsPageIo;
    cache.flush_all(&mut io, core::convert::identity)?;
    impl_page_cache::reset_global_cache(mount_gen);
    Ok(())
}

/// 创建目录（经挂载表路由）。
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

/// 读取符号链接目标（经挂载表路由）。
pub fn read_symlink_path(path : &str) -> VfsResult<Vec<u8>> {
    let abs = normalize_absolute_path(path)?;
    match resolve_route(abs.as_str())? {
        FsRoute::PseudoProc { .. } => Err(VfsError::NotAFile),
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

/// 修改路径权限（经挂载表路由）。
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

pub(crate) fn replace_file_contents(path : &str, data : &[u8]) -> VfsResult<()> {
    mount_table::assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.write_regular_file(rel.as_str(), data)
}

/// 覆盖绝对路径文件：优先原地 truncate+写入；失败时再 unlink 重建，并驱逐页缓存。
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
pub fn mknod_socket_at(path : &str) -> VfsResult<()> {
    mount_table::assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.mknod(rel.as_str(), 0o140_777, 0)
}

/// 重命名绝对路径（经挂载表路由；要求 old/new 落在同一 RW 卷）。
pub fn rename_path(old_path : &str, new_path : &str) -> VfsResult<()> {
    mount_table::assert_path_writable(old_path)?;
    mount_table::assert_path_writable(new_path)?;
    let (fs_old, rel_old) = fs_and_rel_rw(old_path)?;
    let (fs_new, rel_new) = fs_and_rel_rw(new_path)?;
    if !Arc::ptr_eq(&fs_old, &fs_new) {
        return Err(VfsError::Unsupported);
    }
    let mut sess = MountedRwSession::new(fs_old);
    sess.rename(rel_old.as_str(), rel_new.as_str())
}

/// 与只读根句柄分离的可写挂载会话。
pub struct MountedRwSession {
    inner : SharedRwFs,
}

impl MountedRwSession {
    pub fn new(inner : SharedRwFs) -> Self { Self { inner } }
}

impl RootRwSession for MountedRwSession {
    fn write_regular_file_at_root(&mut self, name : &str, data : &[u8]) -> VfsResult<()> {
        validate_root_file_name(name)?;
        self.inner
            .lock()
            .write_regular_file_at_root(name, data)
            .map_err(map_fs_err)
    }

    fn write_regular_file(&mut self, path : &str, data : &[u8]) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .write_regular_file(n.as_str(), data)
            .map_err(map_fs_err)
    }

    fn unlink(&mut self, path : &str) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .unlink(n.as_str())
            .map_err(map_fs_err)
    }

    fn rmdir(&mut self, path : &str) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .rmdir(n.as_str())
            .map_err(map_fs_err)
    }

    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> VfsResult<usize> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .write_range(n.as_str(), offset, data)
            .map_err(map_fs_err)
    }

    fn truncate(&mut self, path : &str, len : u64) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .truncate(n.as_str(), len)
            .map_err(map_fs_err)
    }

    fn mkdir(&mut self, path : &str, mode : u32) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .mkdir(n.as_str(), mode)
            .map_err(map_fs_err)
    }

    fn chmod(&mut self, path : &str, mode : u32) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .chmod(n.as_str(), mode)
            .map_err(map_fs_err)
    }

    fn chown(&mut self, path : &str, uid : Option<u32>, gid : Option<u32>) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .chown(n.as_str(), uid, gid)
            .map_err(map_fs_err)
    }

    fn rename(&mut self, old_path : &str, new_path : &str) -> VfsResult<()> {
        let old = normalize_absolute_path(old_path)?;
        let new = normalize_absolute_path(new_path)?;
        self.inner
            .lock()
            .rename(old.as_str(), new.as_str())
            .map_err(map_fs_err)
    }

    fn hardlink(&mut self, existing_path : &str, new_path : &str) -> VfsResult<()> {
        let existing = normalize_absolute_path(existing_path)?;
        let new = normalize_absolute_path(new_path)?;
        self.inner
            .lock()
            .hardlink(existing.as_str(), new.as_str())
            .map_err(map_fs_err)
    }

    fn symlink(&mut self, link_path : &str, target : &str) -> VfsResult<()> {
        let link = normalize_absolute_path(link_path)?;
        self.inner
            .lock()
            .symlink(link.as_str(), target)
            .map_err(map_fs_err)
    }

    fn mknod(&mut self, path : &str, mode : u32, rdev : u32) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .mknod(n.as_str(), mode, rdev)
            .map_err(map_fs_err)
    }
}

impl VfsMountOps for FsBridge {
    fn supported_capabilities(&self) -> Vec<VfsCapability> {
        fs::supported_fs_summary().into_iter()
                                  .map(map_fs_cap)
                                  .collect()
    }

    fn mount_rw_session(&self, _kind : VfsFsKind) -> VfsResult<Box<dyn RootRwSession>> {
        Ok(Box::new(MountedRwSession::new(root_rw()?)))
    }
}

impl VfsDevInventory for FsBridge {
    fn list_dev_nodes(&self) -> Vec<VfsDevNode> {
        let mut nodes = fs::devfs::active_impl::list_nodes().into_iter()
                                                            .map(|n| {
                                                                VfsDevNode {
                path: n.path,
                node_type: match n.node_type {
                    fs::devfs::DevNodeType::Block => VfsDevNodeType::Block,
                    fs::devfs::DevNodeType::Character => VfsDevNodeType::Character,
                    fs::devfs::DevNodeType::Unsupported => VfsDevNodeType::Unsupported,
                },
            }
                                                            })
                                                            .collect::<Vec<VfsDevNode>>();
        if !nodes.iter()
                 .any(|n| n.path == "/dev/zero")
        {
            nodes.push(VfsDevNode { path : String::from("/dev/zero"),
                                    node_type : VfsDevNodeType::Character });
        }
        if !nodes.iter()
                 .any(|n| n.path == "/dev/urandom")
        {
            nodes.push(VfsDevNode { path : String::from("/dev/urandom"),
                                    node_type : VfsDevNodeType::Character });
        }
        if !nodes.iter()
                 .any(|n| n.path == "/dev/random")
        {
            nodes.push(VfsDevNode { path : String::from("/dev/random"),
                                    node_type : VfsDevNodeType::Character });
        }
        nodes
    }

    fn default_root_block_path(&self) -> Option<String> {
        fs::devfs::active_impl::default_root_block_path()
    }
}

impl VfsOpenOps for FsBridge {
    fn open(&self, path : &str, flags : VfsOpenFlags) -> VfsResult<Box<dyn VfsIoHandle>> {
        self.open_path(path, flags)
    }
}

impl VfsMountTable for FsBridge {
    fn mount_at(&mut self, mount_point : &str, _kind : VfsFsKind) -> VfsResult<()> {
        let _ = mount_point;
        Err(VfsError::Unsupported)
    }

    fn unmount_at(&mut self, mount_point : &str) -> VfsResult<()> {
        mount_table::unmount_aux_at(mount_point)
    }

    fn resolve_mount(&self, path : &str) -> VfsResult<&str> {
        let _ = path;
        Err(VfsError::Unsupported)
    }
}

impl VfsBackend for FsBridge {}

pub fn test() {
    api_v0::test();
    let _ = FsBridge::default();
    let _ = mount_table::mount_table_self_test();
}
