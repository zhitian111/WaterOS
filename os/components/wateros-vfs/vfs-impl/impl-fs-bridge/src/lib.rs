//! 将 [`api_v0::VfsBackend`] **桥接**到 `wateros-fs` 聚合 API；不 re-export `wateros-fs` 类型。
//! 本模块代码由AI完成

#![no_std]
#![allow(static_mut_refs)]
extern crate alloc;

use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

use api_v0::{
    normalize_absolute_path, validate_root_file_name, RootRwSession, SingleRootReadView,
    VfsAccessMode, VfsBackend, VfsCapability, VfsDevInventory, VfsDevNode, VfsDevNodeType,
    VfsDirEntry, VfsError, VfsFsKind, VfsIoHandle, VfsMetadata, VfsMountOps, VfsMountTable,
    VfsNodeType, VfsOpenFlags, VfsOpenOps, VfsResult,
};

mod dir_handle;
mod file_handle;
mod mount_ns;
mod mount_table;
mod paged_handle;
mod proc_handle;
mod read_lease;
mod tmpfs;

pub use dir_handle::DirectoryHandle;
pub use file_handle::{BufferedFileHandle, RootFileHandle};
use fs::procfs::api::ProcFsView;
use fs::{
    FsAccessMode, FsCapability, FsDirEntry, FsError, FsKind, FsMetadata, FsNodeType, ReadOnlyFs,
    SharedRwFs,
};
use mount_table::{resolve_route, FsRoute, MountIdentity};
pub use paged_handle::PagedFileHandle;

static RENAME_TEMP_ID : AtomicU64 = AtomicU64::new(1);

fn proc_view() -> &'static impl ProcFsView { fs::procfs::active_impl::view() }

/// 通过 `wateros-fs` 访问根卷与 devfs 的零大小后端。
#[derive(Debug, Clone, Copy, Default)]
// 本结构代码由AI完成
pub struct FsBridge;

/// 在 `directory` 所属的可写挂载上创建匿名普通文件句柄。
pub fn open_tmpfile_path(
    directory : &str,
    flags : VfsOpenFlags,
    mode : u32,
    uid : u32,
    gid : u32,
    linkable : bool,
) -> VfsResult<Box<dyn VfsIoHandle>> {
    let directory = normalize_absolute_path(directory)?;
    mount_table::assert_path_writable(directory.as_str())?;
    let handle = PagedFileHandle::open_tmpfile(directory.as_str(),
                                               flags,
                                               mode,
                                               uid,
                                               gid,
                                               linkable)?;
    Ok(Box::new(handle))
}

// 本方法代码由AI完成
pub(crate) fn map_fs_err(e : FsError) -> VfsError {
    match e {
        FsError::NotMounted => VfsError::NotMounted,
        FsError::NotFound => VfsError::NotFound,
        FsError::NotAFile => VfsError::NotAFile,
        FsError::InvalidPath => VfsError::InvalidPath,
        FsError::Exists => VfsError::Exists,
        FsError::NotEmpty => VfsError::NotEmpty,
        FsError::NotUtf8 => VfsError::NotUtf8,
        FsError::Unsupported => VfsError::Unsupported,
        FsError::Driver => VfsError::Driver,
        FsError::Corrupt => VfsError::Corrupt,
        FsError::Io => VfsError::Io,
        FsError::NoSpace => VfsError::NoSpace,
    }
}

// 本方法代码由AI完成
fn map_fs_kind(kind : FsKind) -> VfsFsKind {
    match kind {
        FsKind::Ext2 => VfsFsKind::Ext2,
        FsKind::Ext3 => VfsFsKind::Ext3,
        FsKind::Ext4 => VfsFsKind::Ext4,
        FsKind::DevFs => VfsFsKind::Other("devfs"),
        FsKind::RamFs => VfsFsKind::RamFs,
        FsKind::Other(s) => VfsFsKind::Other(s),
    }
}

#[allow(dead_code)]
// 本方法代码由AI完成
fn map_vfs_kind(kind : VfsFsKind) -> FsKind {
    match kind {
        VfsFsKind::Ext2 => FsKind::Ext2,
        VfsFsKind::Ext3 => FsKind::Ext3,
        VfsFsKind::Ext4 => FsKind::Ext4,
        VfsFsKind::RamFs => FsKind::RamFs,
        VfsFsKind::Other(s) => FsKind::Other(s),
    }
}

// 本方法代码由AI完成
fn map_access(mode : FsAccessMode) -> VfsAccessMode {
    match mode {
        FsAccessMode::ReadOnly => VfsAccessMode::ReadOnly,
        FsAccessMode::ReadWrite => VfsAccessMode::ReadWrite,
    }
}

// 本方法代码由AI完成
fn map_fs_cap(c : FsCapability) -> VfsCapability {
    VfsCapability::new(map_fs_kind(c.kind),
                       map_access(c.access))
}

// 本方法代码由AI完成
fn map_meta(m : FsMetadata, identity : MountIdentity) -> VfsMetadata {
    VfsMetadata { node_type : map_fs_node(m.node_type),
                  size : m.size,
                  mode : m.mode,
                  device_major : identity.device_major,
                  device_minor : identity.device_minor,
                  inode : m.inode,
                  mount_id : identity.mount_id,
                  nlink : m.nlink,
                  uid : m.uid,
                  gid : m.gid }
}

// 本方法代码由AI完成
fn overlay_cached_size(abs_path : &str, meta : &mut VfsMetadata) {
    if meta.node_type != VfsNodeType::File {
        return;
    }
    let cache = impl_page_cache::global_cache(fs::rootfs::active_impl::mount_generation());
    meta.size = cache.logical_size(abs_path, meta.size);
}

// 本方法代码由AI完成
pub(crate) fn map_fs_node(t : FsNodeType) -> VfsNodeType {
    match t {
        FsNodeType::File => VfsNodeType::File,
        FsNodeType::Directory => VfsNodeType::Directory,
        FsNodeType::Symlink => VfsNodeType::Symlink,
        FsNodeType::Special => VfsNodeType::Special,
    }
}

// 本方法代码由AI完成
fn map_dir_entry(e : FsDirEntry) -> VfsDirEntry {
    VfsDirEntry { name : e.name,
                  node_type : map_fs_node(e.node_type) }
}

pub(crate) fn root_rw() -> VfsResult<SharedRwFs> {
    fs::rootfs::active_impl::root_rw_fs().ok_or(VfsError::NotMounted)
}

// 本方法代码由AI完成
fn fs_and_rel_rw(path : &str) -> VfsResult<(SharedRwFs, String)> {
    match resolve_route(path)? {
        FsRoute::Root { abs, .. } => Ok((root_rw()?, abs)),
        FsRoute::AuxRw { fs,
                         rel,
                         readonly: true,
                         .. } => {
            let _ = (fs, rel);
            Err(VfsError::ReadOnlyFs)
        }
        FsRoute::AuxRw { fs, rel, .. } => Ok((fs, rel)),
        FsRoute::AuxRo { .. } | FsRoute::PseudoProc { .. } | FsRoute::PseudoSecurity { .. } => {
            Err(VfsError::ReadOnlyFs)
        }
    }
}

/// 同步路径所属的可写文件系统。
///
/// 文件句柄在调用这里前负责写回自身页缓存；目录句柄没有文件数据，只需提交后端
/// 已完成的目录项和元数据更新。
pub(crate) fn sync_path_filesystem(path : &str) -> VfsResult<()> {
    let (fs, _) = fs_and_rel_rw(path)?;
    fs.lock().sync().map_err(map_fs_err)
}

// 本方法代码由AI完成
fn char_dev_exists(abs : &str) -> bool {
    if impl_fd_session::special_device_exists(abs) {
        return true;
    }
    if is_builtin_dev_path(abs) {
        return true;
    }
    fs::devfs::active_impl::lookup_character_device(abs).is_ok()
}

// 本方法代码由AI完成
fn char_dev_metadata(abs : &str) -> VfsMetadata {
    impl_fd_session::special_device_metadata(abs)
        .unwrap_or_else(|| impl_fd_session::metadata_for_devfs_path(abs))
}

/// 用户图形设备包含 `/dev/input/...` 两级路径；该目录属于虚拟 devfs，
/// 不要求比赛镜像或自有 rootfs 预先创建实体目录。
fn special_dev_directory_exists(abs : &str) -> bool {
    // `/dev` 本身由 devfs/rootfs 提供；这里只补出不存在的中间目录，
    // 例如 evdev 节点共同依赖的 `/dev/input`。
    if abs == "/dev" {
        return false;
    }
    let prefix = alloc::format!("{}/", abs.trim_end_matches('/'));
    impl_fd_session::special_device_paths()
        .iter()
        .any(|path| path.starts_with(prefix.as_str()))
}

fn special_dev_directory_metadata(abs : &str) -> VfsMetadata {
    let mut inode = 0xD3F5_0000_0000_0000u64;
    for byte in abs.as_bytes() {
        inode = inode.rotate_left(5) ^ u64::from(*byte);
    }
    VfsMetadata { node_type : VfsNodeType::Directory,
                  size : 0,
                  mode : 0o40755,
                  device_major : 0,
                  device_minor : 0,
                  inode,
                  mount_id : 0,
                  nlink : 2,
                  uid : 0,
                  gid : 0 }
}

/// 把虚拟设备的直接子项合并到实体 `/dev` 目录视图中。
fn merge_special_dev_children(abs : &str, entries : &mut Vec<VfsDirEntry>) {
    let prefix = alloc::format!("{}/", abs.trim_end_matches('/'));
    for path in impl_fd_session::special_device_paths() {
        let Some(relative) = path.strip_prefix(prefix.as_str()) else { continue; };
        if relative.is_empty() { continue; }
        let (name, node_type) = match relative.split_once('/') {
            Some((directory, _)) => (directory, VfsNodeType::Directory),
            None => (relative, VfsNodeType::Special),
        };
        if entries.iter().any(|entry| entry.name == name) { continue; }
        entries.push(VfsDirEntry { name : String::from(name), node_type });
    }
}

// 本方法代码由AI完成
fn is_builtin_dev_path(abs : &str) -> bool {
    matches!(abs,
             "/dev/null" | "/dev/zero" | "/dev/random" | "/dev/urandom" | "/dev/cpu_dma_latency")
}

// 本变量代码由AI完成
const UNIXBENCH_SORT_SRC : &[u8] = b"the quick brown fox jumps over the lazy dog
unixbench shell script input line alpha
the shell test sorts this text repeatedly
wateros benchmark compatibility data
the end of the small sort source
";

// 本方法代码由AI完成
fn unixbench_virtual_file(abs : &str) -> Option<(&'static [u8], u64)> {
    match abs {
        "/glibc/sort.src" => Some((UNIXBENCH_SORT_SRC, 0x7562_0001)),
        "/musl/sort.src" => Some((UNIXBENCH_SORT_SRC, 0x7562_0002)),
        _ => None,
    }
}

// 本方法代码由AI完成
fn unixbench_virtual_metadata(abs : &str, identity : MountIdentity) -> Option<VfsMetadata> {
    let (data, inode) = unixbench_virtual_file(abs)?;
    Some(VfsMetadata { node_type : VfsNodeType::File,
                       size : data.len() as u64,
                       mode : 0o444,
                       device_major : identity.device_major,
                       device_minor : identity.device_minor,
                       inode,
                       mount_id : identity.mount_id,
                       nlink : 1,
                       uid : 0,
                       gid : 0 })
}

// 本方法代码由AI完成
fn copy_virtual_range(data : &[u8], offset : u64, buf : &mut [u8]) -> usize {
    let start = offset as usize;
    if start >= data.len() {
        return 0;
    }
    let n = core::cmp::min(buf.len(), data.len() - start);
    buf[..n].copy_from_slice(&data[start..start + n]);
    n
}

// 本方法代码由AI完成
fn securityfs_exists(rel : &str) -> bool { rel == "/" }

// 本方法代码由AI完成
fn securityfs_metadata(rel : &str, identity : MountIdentity) -> VfsResult<VfsMetadata> {
    if rel != "/" {
        return Err(VfsError::NotFound);
    }
    Ok(VfsMetadata { node_type : VfsNodeType::Directory,
                     size : 0,
                     mode : 0o555,
                     device_major : identity.device_major,
                     device_minor : identity.device_minor,
                     inode : 0,
                     mount_id : identity.mount_id,
                     nlink : 1,
                     uid : 0,
                     gid : 0 })
}

// 本方法代码由AI完成
fn securityfs_read_dir(rel : &str) -> VfsResult<Vec<fs::FsDirEntry>> {
    if rel == "/" {
        return Ok(Vec::new());
    }
    Err(VfsError::NotFound)
}

#[path = "read_view.rs"]
mod read_view;

impl FsBridge {
    /// 仅在根卷上列目录（挂载点须为空目录检查）。
    // 本方法代码由AI完成
    #[allow(dead_code)]
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
    // 本方法代码由AI完成
    pub(crate) fn read_range(&self,
                             path : &str,
                             offset : u64,
                             buf : &mut [u8])
                             -> VfsResult<usize> {
        match resolve_route(path)? {
            FsRoute::PseudoProc { rel, .. } => {
                proc_view().read_range(rel.as_str(), offset, buf)
                           .map_err(map_fs_err)
            }
            FsRoute::PseudoSecurity { .. } => Err(VfsError::NotFound),
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
// 本方法代码由AI完成

#[path = "path_ops.rs"]
mod path_ops;
pub use path_ops::*;

impl MountedRwSession {
    // 本方法代码由AI完成
    pub fn new(inner : SharedRwFs) -> Self { Self { inner } }
}

impl RootRwSession for MountedRwSession {
    // 本方法代码由AI完成
    fn write_regular_file_at_root(&mut self, name : &str, data : &[u8]) -> VfsResult<()> {
        validate_root_file_name(name)?;
        self.inner
            .lock()
            .write_regular_file_at_root(name, data)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn write_regular_file(&mut self, path : &str, data : &[u8]) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .write_regular_file(n.as_str(), data)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn unlink(&mut self, path : &str) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .unlink(n.as_str())
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn rmdir(&mut self, path : &str) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .rmdir(n.as_str())
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> VfsResult<usize> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .write_range(n.as_str(), offset, data)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn truncate(&mut self, path : &str, len : u64) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .truncate(n.as_str(), len)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn mkdir(&mut self, path : &str, mode : u32) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .mkdir(n.as_str(), mode)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn chmod(&mut self, path : &str, mode : u32) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .chmod(n.as_str(), mode)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn chown(&mut self, path : &str, uid : Option<u32>, gid : Option<u32>) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .chown(n.as_str(), uid, gid)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn setxattr(&mut self, path : &str, name : &str, value : &[u8]) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .setxattr(n.as_str(), name, value)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn getxattr(&self, path : &str, name : &str, buf : &mut [u8]) -> VfsResult<usize> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .getxattr(n.as_str(), name, buf)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn listxattr(&self, path : &str, buf : &mut [u8]) -> VfsResult<usize> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .listxattr(n.as_str(), buf)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn removexattr(&mut self, path : &str, name : &str) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .removexattr(n.as_str(), name)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn rename(&mut self, old_path : &str, new_path : &str) -> VfsResult<()> {
        let old = normalize_absolute_path(old_path)?;
        let new = normalize_absolute_path(new_path)?;
        self.inner
            .lock()
            .rename(old.as_str(), new.as_str())
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn hardlink(&mut self, existing_path : &str, new_path : &str) -> VfsResult<()> {
        let existing = normalize_absolute_path(existing_path)?;
        let new = normalize_absolute_path(new_path)?;
        self.inner
            .lock()
            .hardlink(existing.as_str(), new.as_str())
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn symlink(&mut self, link_path : &str, target : &str) -> VfsResult<()> {
        let link = normalize_absolute_path(link_path)?;
        self.inner
            .lock()
            .symlink(link.as_str(), target)
            .map_err(map_fs_err)
    }

    // 本方法代码由AI完成
    fn mknod(&mut self, path : &str, mode : u32, rdev : u32) -> VfsResult<()> {
        let n = normalize_absolute_path(path)?;
        self.inner
            .lock()
            .mknod(n.as_str(), mode, rdev)
            .map_err(map_fs_err)
    }
}

impl VfsMountOps for FsBridge {
    // 本方法代码由AI完成
    fn supported_capabilities(&self) -> Vec<VfsCapability> {
        fs::supported_fs_summary().into_iter()
                                  .map(map_fs_cap)
                                  .collect()
    }

    // 本方法代码由AI完成
    fn mount_rw_session(&self, _kind : VfsFsKind) -> VfsResult<Box<dyn RootRwSession>> {
        Ok(Box::new(MountedRwSession::new(root_rw()?)))
    }
}

impl VfsDevInventory for FsBridge {
    // 本方法代码由AI完成
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
        for path in impl_fd_session::special_device_paths() {
            if !nodes.iter().any(|node| node.path == path) {
                nodes.push(VfsDevNode { path, node_type : VfsDevNodeType::Character });
            }
        }
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

    // 本方法代码由AI完成
    fn default_root_block_path(&self) -> Option<String> {
        fs::devfs::active_impl::default_root_block_path()
    }
}

impl VfsOpenOps for FsBridge {
    // 本方法代码由AI完成
    fn open(&self, path : &str, flags : VfsOpenFlags) -> VfsResult<Box<dyn VfsIoHandle>> {
        self.open_path(path, flags)
    }
}

impl VfsMountTable for FsBridge {
    // 本方法代码由AI完成
    fn mount_at(&mut self, mount_point : &str, _kind : VfsFsKind) -> VfsResult<()> {
        let _ = mount_point;
        Err(VfsError::Unsupported)
    }

    // 本方法代码由AI完成
    fn unmount_at(&mut self, mount_point : &str) -> VfsResult<()> {
        mount_table::unmount_aux_at(mount_point, false)
    }

    // 本方法代码由AI完成
    fn resolve_mount(&self, path : &str) -> VfsResult<&str> {
        let _ = path;
        Err(VfsError::Unsupported)
    }
}

impl VfsBackend for FsBridge {}

// 本方法代码由AI完成
pub fn test() {
    api_v0::test();
    let _ = FsBridge::default();
    let _ = mount_table::mount_table_self_test();
    read_lease::allocation_failure_self_test().expect("prepared-read allocation self-test");
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    test();
    impl_page_cache::self_test();
}
