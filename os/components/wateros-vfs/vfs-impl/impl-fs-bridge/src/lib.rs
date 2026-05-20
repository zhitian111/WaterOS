//! 将 [`api_v0::VfsBackend`] **桥接**到 `wateros-fs` 聚合 API；不 re-export `wateros-fs` 类型。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, string::String, vec::Vec};

use api_v0::{
    RootRwSession, SingleRootReadView, VfsAccessMode, VfsBackend, VfsCapability, VfsDevInventory,
    VfsDevNode, VfsDevNodeType, VfsDirEntry, VfsError, VfsFsKind, VfsIoHandle, VfsMetadata,
    VfsMountOps, VfsMountTable, VfsNodeType, VfsOpenFlags, VfsOpenOps, VfsResult,
    normalize_absolute_path, validate_root_file_name,
};

mod file_handle;

pub use file_handle::RootFileHandle;
use fs::{FsAccessMode, FsCapability, FsDirEntry, FsError, FsKind, FsMetadata, FsNodeType, SharedRwFs};

/// 通过 `wateros-fs` 访问根卷与 devfs 的零大小后端。
#[derive(Debug, Clone, Copy, Default)]
pub struct FsBridge;

pub(crate) fn map_fs_err(e: FsError) -> VfsError {
    match e {
        FsError::NotMounted => VfsError::NotMounted,
        FsError::NotFound => VfsError::NotFound,
        FsError::NotAFile => VfsError::NotAFile,
        FsError::InvalidPath => VfsError::InvalidPath,
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

impl SingleRootReadView for FsBridge {
    fn exists(&self, path: &str) -> VfsResult<bool> {
        let n = normalize_absolute_path(path)?;
        let Some(fs) = fs::rootfs::active_impl::root_fs() else {
            return Err(VfsError::NotMounted);
        };
        fs.lock().exists(n.as_str()).map_err(map_fs_err)
    }

    fn metadata(&self, path: &str) -> VfsResult<VfsMetadata> {
        let n = normalize_absolute_path(path)?;
        let Some(fs) = fs::rootfs::active_impl::root_fs() else {
            return Err(VfsError::NotMounted);
        };
        fs.lock().metadata(n.as_str()).map_err(map_fs_err).map(map_meta)
    }

    fn read(&self, path: &str) -> VfsResult<Vec<u8>> {
        let n = normalize_absolute_path(path)?;
        let Some(fs) = fs::rootfs::active_impl::root_fs() else {
            return Err(VfsError::NotMounted);
        };
        fs.lock().read(n.as_str()).map_err(map_fs_err)
    }

    fn read_dir(&self, path: &str) -> VfsResult<Vec<VfsDirEntry>> {
        let n = normalize_absolute_path(path)?;
        let Some(fs) = fs::rootfs::active_impl::root_fs() else {
            return Err(VfsError::NotMounted);
        };
        fs.lock()
            .read_dir(n.as_str())
            .map_err(map_fs_err)
            .map(|v| v.into_iter().map(map_dir_entry).collect())
    }

    fn boot_dump_all_paths(&self) {
        if let Some(fs) = fs::rootfs::active_impl::root_fs() {
            fs.lock().boot_dump_all_paths();
        }
    }
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
}

impl VfsMountOps for FsBridge {
    fn supported_capabilities(&self) -> Vec<VfsCapability> {
        fs::supported_fs_summary()
            .into_iter()
            .map(map_fs_cap)
            .collect()
    }

    fn mount_rw_session(&self, kind: VfsFsKind) -> VfsResult<Box<dyn RootRwSession>> {
        let fs_kind = map_vfs_kind(kind);
        let imp = fs::pick_fs_impl(fs_kind, FsAccessMode::ReadWrite).ok_or(VfsError::Unsupported)?;
        let dev_path = fs::rootfs::active_impl::current_root_device_path()
            .ok_or(VfsError::NotMounted)?;
        let dev = fs::devfs::active_impl::lookup_block_device(dev_path.as_str())
            .map_err(map_fs_err)?;
        let rw = imp.mount_rw(dev).map_err(map_fs_err)?;
        Ok(Box::new(MountedRwSession::new(rw)))
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

impl VfsMountTable for FsBridge {}

impl VfsBackend for FsBridge {}

pub fn test() {
    api_v0::test();
    let _ = FsBridge::default();
}
