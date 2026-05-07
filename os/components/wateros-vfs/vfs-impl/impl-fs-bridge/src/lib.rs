//! 将 [`api_v0`] 的 VFS trait **桥接**到 `wateros-fs` 聚合 API（根卷只读、`SharedRwFs` 可写挂载、devfs 块设备枚举等）。
//!
//! 本 crate **不** 重新定义 ext4 等格式逻辑，只做错误/元数据映射与路径委托；未挂载或缺设备时行为与 `api_v0` 错误语义一致。

#![no_std]
extern crate alloc;

use alloc::{string::String, vec::Vec};

pub use api_v0::{
    normalize_absolute_path, NormalizedPath, RootRwSession, SingleRootReadView, VfsError,
    VfsMetadata, VfsNodeType, VfsResult,
};

pub use fs::{devfs, rootfs, FsAccessMode, FsCapability, FsImpl, FsKind};

use fs::{FsError, FsMetadata, FsNodeType, SharedRwFs};

/// 内核侧通过 `wateros-fs` 聚合 API 访问根卷与 devfs 的零大小桥接器。
#[derive(Debug, Clone, Copy, Default)]
pub struct FsBridge;

fn map_fs_err(e: FsError) -> VfsError {
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

fn map_meta(m: FsMetadata) -> VfsMetadata {
    VfsMetadata {
        node_type: match m.node_type {
            FsNodeType::File => VfsNodeType::File,
            FsNodeType::Directory => VfsNodeType::Directory,
            FsNodeType::Symlink => VfsNodeType::Symlink,
            FsNodeType::Special => VfsNodeType::Special,
        },
        size: m.size,
        mode: m.mode,
    }
}

impl SingleRootReadView for FsBridge {
    fn exists(&self, path: &str) -> VfsResult<bool> {
        let n = normalize_absolute_path(path)?;
        let Some(fs) = rootfs::active_impl::root_fs() else {
            return Err(VfsError::NotMounted);
        };
        fs.lock().exists(n.as_str()).map_err(map_fs_err)
    }

    fn metadata(&self, path: &str) -> VfsResult<VfsMetadata> {
        let n = normalize_absolute_path(path)?;
        let Some(fs) = rootfs::active_impl::root_fs() else {
            return Err(VfsError::NotMounted);
        };
        fs.lock().metadata(n.as_str()).map_err(map_fs_err).map(map_meta)
    }

    fn read(&self, path: &str) -> VfsResult<Vec<u8>> {
        let n = normalize_absolute_path(path)?;
        let Some(fs) = rootfs::active_impl::root_fs() else {
            return Err(VfsError::NotMounted);
        };
        fs.lock().read(n.as_str()).map_err(map_fs_err)
    }

    fn boot_dump_all_paths(&self) {
        if let Some(fs) = rootfs::active_impl::root_fs() {
            fs.lock().boot_dump_all_paths();
        }
    }
}

/// 与只读根句柄分离的可写挂载会话（独立 `SharedRwFs`）。
pub struct MountedRwSession {
    inner: SharedRwFs,
}

impl MountedRwSession {
    pub fn new(inner: SharedRwFs) -> Self { Self { inner } }
}

impl RootRwSession for MountedRwSession {
    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> VfsResult<()> {
        validate_root_file_name(name)?;
        self.inner
            .lock()
            .write_regular_file_at_root(name, data)
            .map_err(map_fs_err)
    }
}

/// 根目录下文件名不得包含 `/` 或为空（与 `ReadWriteFs` 约定一致）。
pub fn validate_root_file_name(name: &str) -> VfsResult<()> {
    if name.is_empty() || name.contains('/') {
        return Err(VfsError::InvalidPath);
    }
    Ok(())
}

/// 扁平化已注册 `FsImpl` 的能力（委托 `fs::supported_fs_summary`）。
pub fn supported_fs_capabilities() -> Vec<FsCapability> { fs::supported_fs_summary() }

/// 在注册表中选择 `(kind, mode)` 对应实现（委托 `fs::pick_fs_impl`）。
pub fn pick_fs_impl(kind: FsKind, mode: FsAccessMode) -> Option<&'static dyn FsImpl> {
    fs::pick_fs_impl(kind, mode)
}

pub fn mount_rw_session(kind: FsKind) -> VfsResult<MountedRwSession> {
    let imp = fs::pick_fs_impl(kind, FsAccessMode::ReadWrite).ok_or(VfsError::Unsupported)?;
    let dev_path = rootfs::active_impl::current_root_device_path().ok_or(VfsError::NotMounted)?;
    let dev = devfs::active_impl::lookup_block_device(dev_path.as_str()).map_err(map_fs_err)?;
    let rw = imp.mount_rw(dev).map_err(map_fs_err)?;
    Ok(MountedRwSession::new(rw))
}

/// RW 写入后通过只读根句柄读回校验（语义对齐 `wateros-fs` 聚合层 `test()` 中 RW 段）。
pub fn rw_write_root_verify_via_ro(kind: FsKind, name: &str, data: &[u8]) -> VfsResult<()> {
    validate_root_file_name(name)?;
    let mut session = mount_rw_session(kind)?;
    session.write_regular_file_at_root(name, data)?;
    let ro = rootfs::active_impl::root_fs().ok_or(VfsError::NotMounted)?;
    let mut path = String::from("/");
    path.push_str(name);
    let n = normalize_absolute_path(path.as_str())?;
    let bytes = ro.lock().read(n.as_str()).map_err(map_fs_err)?;
    if bytes.as_slice() == data {
        Ok(())
    } else {
        Err(VfsError::Io)
    }
}

/// 枚举 devfs 节点（委托 `devfs::active_impl::list_nodes`）。
pub fn list_dev_nodes() -> Vec<devfs::DevNode> { devfs::active_impl::list_nodes() }

pub fn default_root_block_path() -> Option<String> { devfs::active_impl::default_root_block_path() }

pub fn test() {
    api_v0::test();
    let _ = FsBridge::default();
}
