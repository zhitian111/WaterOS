//! 将 [`api_v0`] 的 VFS trait **桥接**到 `wateros-fs` 聚合 API（根卷只读、`SharedRwFs` 可写挂载、devfs 块设备枚举等）。
//!
//! 本 crate **不** 重新定义 ext4 等格式逻辑，只做错误/元数据映射与路径委托；未挂载或缺设备时行为与 `api_v0` 错误语义一致。

#![no_std]
extern crate alloc;

use alloc::{string::String, vec::Vec};

pub use api_v0::{
    normalize_absolute_path, NormalizedPath, RootRwSession, SingleRootReadView, VfsDirEntry,
    VfsError, VfsMetadata, VfsNodeType, VfsResult,
};

pub use fs::{devfs, rootfs, FsAccessMode, FsCapability, FsImpl, FsKind};

use fs::{FsDirEntry, FsError, FsMetadata, FsNodeType, SharedRwFs};

/// 内核侧通过 `wateros-fs` 聚合 API 访问根卷与 devfs 的零大小桥接器。
#[derive(Debug, Clone, Copy, Default)]
pub struct FsBridge;

// `wateros-fs` 错误与 VFS 枚举一一对应，便于上层只依赖 `api-v0`。
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

// 节点类型与数值字段原样映射，不附加 VFS 侧语义。
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

// 只读路径：规范化后委托 `rootfs::active_impl`；无活动根卷时与 API 约定一致返回未挂载。
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

    fn read_dir(&self, path: &str) -> VfsResult<Vec<VfsDirEntry>> {
        let n = normalize_absolute_path(path)?;
        let Some(fs) = rootfs::active_impl::root_fs() else {
            return Err(VfsError::NotMounted);
        };
        fs.lock()
            .read_dir(n.as_str())
            .map_err(map_fs_err)
            .map(|v| v.into_iter().map(map_dir_entry).collect())
    }

    fn boot_dump_all_paths(&self) {
        // 无根卷时静默跳过，避免启动调试路径在非挂载配置下 panic。
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
    /// 由 [`mount_rw_session`] 或测试在拿到 `SharedRwFs` 后包装为 [`RootRwSession`]。
    pub fn new(inner: SharedRwFs) -> Self { Self { inner } }
}

impl RootRwSession for MountedRwSession {
    // 文件名校验与聚合 FS 锁顺序与 `wateros-fs` 侧约定一致。
    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> VfsResult<()> {
        validate_root_file_name(name)?;
        self.inner
            .lock()
            .write_regular_file_at_root(name, data)
            .map_err(map_fs_err)
    }

    fn unlink(&mut self, path: &str) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner.lock().unlink(n.as_str()).map_err(map_fs_err)
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

/// 为指定 [`FsKind`] 挂载 RW 会话：要求存在 RW 能力实现、当前根块设备路径及 devfs 中对应块设备。
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

/// 启动策略给出的默认根块设备路径（若未配置则为 `None`）。
pub fn default_root_block_path() -> Option<String> { devfs::active_impl::default_root_block_path() }

/// 桥接 crate 自检：复用 `api-v0` 路径测试并构造默认 [`FsBridge`]（不假设已挂载卷）。
pub fn test() {
    api_v0::test();
    let _ = FsBridge::default();
}
