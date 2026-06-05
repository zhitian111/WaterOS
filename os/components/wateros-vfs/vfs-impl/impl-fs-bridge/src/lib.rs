//! 将 [`api_v0::VfsBackend`] **桥接**到 `wateros-fs` 聚合 API；不 re-export `wateros-fs` 类型。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};

use api_v0::{
    RootRwSession, SingleRootReadView, VfsAccessMode, VfsBackend, VfsCapability, VfsDevInventory,
    VfsDevNode, VfsDevNodeType, VfsDirEntry, VfsError, VfsFsKind, VfsIoHandle, VfsMetadata,
    VfsMountOps, VfsMountTable, VfsNodeType, VfsOpenFlags, VfsOpenOps, VfsResult,
    normalize_absolute_path, validate_root_file_name,
};

mod dir_handle;
mod file_handle;
mod mount_table;
mod paged_handle;

pub use dir_handle::DirectoryHandle;
pub use file_handle::{BufferedFileHandle, RootFileHandle};
pub use paged_handle::PagedFileHandle;
use fs::{
    FsAccessMode, FsCapability, FsDirEntry, FsError, FsKind, FsMetadata, FsNodeType, ReadOnlyFs,
    SharedRwFs,
};
use mount_table::{assert_path_writable, resolve_route, FsRoute};

/// 通过 `wateros-fs` 访问根卷与 devfs 的零大小后端。
#[derive(Debug, Clone, Copy, Default)]
pub struct FsBridge;

pub(crate) fn map_fs_err(e: FsError) -> VfsError {
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
    }
}

fn map_fs_kind(kind: FsKind) -> VfsFsKind {
    match kind {
        FsKind::Ext2 => VfsFsKind::Ext2,
        FsKind::Ext3 => VfsFsKind::Ext3,
        FsKind::Ext4 => VfsFsKind::Ext4,
        FsKind::DevFs => VfsFsKind::Other("devfs"),
        FsKind::Other(s) => VfsFsKind::Other(s),
    }
}

fn map_vfs_kind(kind: VfsFsKind) -> FsKind {
    match kind {
        VfsFsKind::Ext2 => FsKind::Ext2,
        VfsFsKind::Ext3 => FsKind::Ext3,
        VfsFsKind::Ext4 => FsKind::Ext4,
        VfsFsKind::Other(s) => FsKind::Other(s),
    }
}

fn map_access(mode: FsAccessMode) -> VfsAccessMode {
    match mode {
        FsAccessMode::ReadOnly => VfsAccessMode::ReadOnly,
        FsAccessMode::ReadWrite => VfsAccessMode::ReadWrite,
    }
}

fn map_fs_cap(c: FsCapability) -> VfsCapability {
    VfsCapability::new(map_fs_kind(c.kind), map_access(c.access))
}

fn map_meta(m: FsMetadata) -> VfsMetadata {
    VfsMetadata {
        node_type: map_fs_node(m.node_type),
        size: m.size,
        mode: m.mode,
    }
}

fn map_fs_node(t: FsNodeType) -> VfsNodeType {
    match t {
        FsNodeType::File => VfsNodeType::File,
        FsNodeType::Directory => VfsNodeType::Directory,
        FsNodeType::Symlink => VfsNodeType::Symlink,
        FsNodeType::Special => VfsNodeType::Special,
    }
}

fn map_dir_entry(e: FsDirEntry) -> VfsDirEntry {
    VfsDirEntry {
        name: e.name,
        node_type: map_fs_node(e.node_type),
    }
}

pub(crate) fn root_rw() -> VfsResult<SharedRwFs> {
    fs::rootfs::active_impl::root_rw_fs().ok_or(VfsError::NotMounted)
}

fn fs_and_rel_rw(path: &str) -> VfsResult<(SharedRwFs, String)> {
    match resolve_route(path)? {
        FsRoute::Root { abs } => Ok((root_rw()?, abs)),
        FsRoute::AuxRw { fs, rel } => Ok((fs, rel)),
        FsRoute::AuxRo { .. } => Err(VfsError::ReadOnlyFs),
    }
}

fn char_dev_exists(abs: &str) -> bool {
    fs::devfs::active_impl::lookup_character_device(abs).is_ok()
}

fn char_dev_metadata(abs: &str) -> VfsMetadata {
    let mode = if matches!(abs, "/dev/misc/rtc" | "/dev/rtc0" | "/dev/rtc") {
        0o20644u16
    } else {
        0o20660u16
    };
    VfsMetadata {
        node_type: VfsNodeType::Special,
        size: 0,
        mode,
    }
}

impl SingleRootReadView for FsBridge {
    fn exists(&self, path: &str) -> VfsResult<bool> {
        let abs = normalize_absolute_path(path)?;
        if char_dev_exists(abs.as_str()) {
            return Ok(true);
        }
        match resolve_route(abs.as_str())? {
            FsRoute::Root { abs } => root_rw()?.lock().exists(abs.as_str()).map_err(map_fs_err),
            FsRoute::AuxRw { fs, rel } => {
                fs.lock().exists(rel.as_str()).map_err(map_fs_err)
            }
            FsRoute::AuxRo { fs, rel } => {
                fs.lock().exists(rel.as_str()).map_err(map_fs_err)
            }
        }
    }

    fn metadata(&self, path: &str) -> VfsResult<VfsMetadata> {
        let abs = normalize_absolute_path(path)?;
        if char_dev_exists(abs.as_str()) {
            return Ok(char_dev_metadata(abs.as_str()));
        }
        let meta = match resolve_route(abs.as_str())? {
            FsRoute::Root { abs } => root_rw()?
                .lock()
                .metadata(abs.as_str())
                .map_err(map_fs_err)?,
            FsRoute::AuxRw { fs, rel } => fs
                .lock()
                .metadata(rel.as_str())
                .map_err(map_fs_err)?,
            FsRoute::AuxRo { fs, rel } => fs
                .lock()
                .metadata(rel.as_str())
                .map_err(map_fs_err)?,
        };
        Ok(map_meta(meta))
    }

    fn read(&self, path: &str) -> VfsResult<Vec<u8>> {
        match resolve_route(path)? {
            FsRoute::Root { abs } => root_rw()?.lock().read(abs.as_str()).map_err(map_fs_err),
            FsRoute::AuxRw { fs, rel } => fs.lock().read(rel.as_str()).map_err(map_fs_err),
            FsRoute::AuxRo { fs, rel } => fs.lock().read(rel.as_str()).map_err(map_fs_err),
        }
    }

    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        FsBridge::read_range(self, path, offset, buf)
    }

    fn read_dir(&self, path: &str) -> VfsResult<Vec<VfsDirEntry>> {
        let entries = match resolve_route(path)? {
            FsRoute::Root { abs } => root_rw()?
                .lock()
                .read_dir(abs.as_str())
                .map_err(map_fs_err)?,
            FsRoute::AuxRw { fs, rel } => fs
                .lock()
                .read_dir(rel.as_str())
                .map_err(map_fs_err)?,
            FsRoute::AuxRo { fs, rel } => fs
                .lock()
                .read_dir(rel.as_str())
                .map_err(map_fs_err)?,
        };
        Ok(entries.into_iter().map(map_dir_entry).collect())
    }

    fn boot_dump_all_paths(&self) {
        // bring-up 单 RW 根卷：启动树打印仍可由 fs 层自检触发。
    }
}

impl FsBridge {
    /// 仅在根卷上列目录（挂载点须为空目录检查）。
    pub(crate) fn read_dir_on_root(path: &str) -> VfsResult<Vec<VfsDirEntry>> {
        let n = normalize_absolute_path(path)?;
        let fs = root_rw()?;
        fs.lock()
            .read_dir(n.as_str())
            .map_err(map_fs_err)
            .map(|v| v.into_iter().map(map_dir_entry).collect())
    }

    /// 从根卷只读句柄按偏移读取（页缓存 miss 路径）。
    pub(crate) fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        match resolve_route(path)? {
            FsRoute::Root { abs } => root_rw()?
                .lock()
                .read_range(abs.as_str(), offset, buf)
                .map_err(map_fs_err),
            FsRoute::AuxRw { fs, rel } => fs
                .lock()
                .read_range(rel.as_str(), offset, buf)
                .map_err(map_fs_err),
            FsRoute::AuxRo { fs, rel } => fs
                .lock()
                .read_range(rel.as_str(), offset, buf)
                .map_err(map_fs_err),
        }
    }
}

/// 将 ext4 块设备挂到根卷内 `mount_point`（须为空目录）。
pub fn mount_ext4_block_at(mount_point: &str, block_dev: &str, readonly: bool) -> VfsResult<()> {
    if readonly {
        let aux = fs::mount_aux_ro_from_block_path(block_dev).map_err(map_fs_err)?;
        mount_table::mount_aux_at_ro(mount_point, aux)
    } else {
        let aux = fs::mount_aux_rw_from_block_path(block_dev).map_err(map_fs_err)?;
        mount_table::mount_aux_at_rw(mount_point, aux)
    }
}

/// 卸载 `mount_point` 上的辅助卷。
pub fn unmount_at(mount_point: &str) -> VfsResult<()> {
    mount_table::unmount_aux_at(mount_point)
}

/// 删除绝对路径（经挂载表路由）。
pub fn unlink_path(path: &str, remove_dir: bool) -> VfsResult<()> {
    assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    if remove_dir {
        sess.rmdir(rel.as_str())
    } else {
        sess.unlink(rel.as_str())
    }
}

/// 创建目录（经挂载表路由）。
pub fn mkdir_path(path: &str, mode: u32) -> VfsResult<()> {
    assert_path_writable(path)?;
    let (fs, rel) = fs_and_rel_rw(path)?;
    let mut sess = MountedRwSession::new(fs);
    sess.mkdir(rel.as_str(), mode)
}

/// 重命名绝对路径（经挂载表路由；要求 old/new 落在同一 RW 卷）。
pub fn rename_path(old_path: &str, new_path: &str) -> VfsResult<()> {
    assert_path_writable(old_path)?;
    assert_path_writable(new_path)?;
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
    inner: SharedRwFs,
}

impl MountedRwSession {
    pub fn new(inner: SharedRwFs) -> Self {
        Self { inner }
    }
}

impl RootRwSession for MountedRwSession {
    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> VfsResult<()> {
        validate_root_file_name(name)?;
        self.inner
            .lock()
            .write_regular_file_at_root(name, data)
            .map_err(map_fs_err)
    }

    fn write_regular_file(&mut self, path: &str, data: &[u8]) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .write_regular_file(n.as_str(), data)
            .map_err(map_fs_err)
    }

    fn unlink(&mut self, path: &str) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner.lock().unlink(n.as_str()).map_err(map_fs_err)
    }

    fn rmdir(&mut self, path: &str) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner.lock().rmdir(n.as_str()).map_err(map_fs_err)
    }

    fn write_range(&mut self, path: &str, offset: u64, data: &[u8]) -> VfsResult<usize> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .write_range(n.as_str(), offset, data)
            .map_err(map_fs_err)
    }

    fn mkdir(&mut self, path: &str, mode: u32) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner.lock().mkdir(n.as_str(), mode).map_err(map_fs_err)
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let old = normalize_absolute_path(old_path)?;
        let new = normalize_absolute_path(new_path)?;
        self.inner
            .lock()
            .rename(old.as_str(), new.as_str())
            .map_err(map_fs_err)
    }
}

impl VfsMountOps for FsBridge {
    fn supported_capabilities(&self) -> Vec<VfsCapability> {
        fs::supported_fs_summary()
            .into_iter()
            .map(map_fs_cap)
            .collect()
    }

    fn mount_rw_session(&self, _kind: VfsFsKind) -> VfsResult<Box<dyn RootRwSession>> {
        Ok(Box::new(MountedRwSession::new(root_rw()?)))
    }
}

impl VfsDevInventory for FsBridge {
    fn list_dev_nodes(&self) -> Vec<VfsDevNode> {
        fs::devfs::active_impl::list_nodes()
            .into_iter()
            .map(|n| VfsDevNode {
                path: n.path,
                node_type: match n.node_type {
                    fs::devfs::DevNodeType::Block => VfsDevNodeType::Block,
                    fs::devfs::DevNodeType::Character => VfsDevNodeType::Character,
                    fs::devfs::DevNodeType::Unsupported => VfsDevNodeType::Unsupported,
                },
            })
            .collect()
    }

    fn default_root_block_path(&self) -> Option<String> {
        fs::devfs::active_impl::default_root_block_path()
    }
}

impl VfsOpenOps for FsBridge {
    fn open(&self, path: &str, flags: VfsOpenFlags) -> VfsResult<Box<dyn VfsIoHandle>> {
        self.open_path(path, flags)
    }
}

impl VfsMountTable for FsBridge {
    fn mount_at(&mut self, mount_point: &str, _kind: VfsFsKind) -> VfsResult<()> {
        let _ = mount_point;
        Err(VfsError::Unsupported)
    }

    fn unmount_at(&mut self, mount_point: &str) -> VfsResult<()> {
        mount_table::unmount_aux_at(mount_point)
    }

    fn resolve_mount(&self, path: &str) -> VfsResult<&str> {
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
