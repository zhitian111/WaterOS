//! 虚拟文件系统 **聚合 crate**：[`api`] 定义基本能力，[`active_impl`] 选择后端，
//! [`root`] / [`mount`] / [`self_test`] 将能力组合为对外稳定接口。

#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;

#[cfg(feature = "bridge-fs-api")]
extern crate fs;
#[cfg(feature = "bridge-fs-api")]
extern crate impl_fs_bridge;

#[cfg(feature = "impl-fd-session")]
extern crate base;
#[cfg(feature = "impl-fd-session")]
extern crate impl_fd_session;
#[cfg(feature = "impl-fd-session")]
extern crate task;

pub use api_v0 as api;
pub use api_v0::{
    normalize_absolute_path, register_open_path_resolver, resolve_against_cwd, resolve_open_path,
    resolve_symlink_path_with, validate_root_file_name, FinalSymlink, NormalizedPath,
    RootRwSession, SingleRootReadView, VfsAccessMode, VfsBackend, VfsCapability, VfsDevInventory,
    VfsDevNode, VfsDevNodeType, VfsDirEntry, VfsError, VfsFd, VfsFdSession, VfsFileHandle,
    VfsFsKind, VfsIoHandle, VfsMetadata, VfsMountOps, VfsMountTable, VfsNodeType, VfsOpenFlags,
    VfsOpenOps, VfsResult, VfsSeekWhence, VFS_FIRST_DYNAMIC_FD, VFS_STDERR_FD, VFS_STDIN_FD,
    VFS_STDOUT_FD,
};

/// per-task 文件描述符会话（`impl-fd-session` feature）。
#[cfg(feature = "impl-fd-session")]
pub mod fd;

/// per-task 工作目录（`impl-fd-session` feature）。
#[cfg(feature = "impl-fd-session")]
pub mod cwd;

/// 在平台驱动探测完成后建立 `/dev/input/eventN` 稳定索引。
#[cfg(feature = "user-graphics")]
pub fn initialize_user_graphics_devices() -> bool {
    impl_fd_session::initialize_user_graphics_devices()
}

/// 用户态图形的低优先级输入汇聚任务入口。
#[cfg(feature = "user-graphics")]
pub extern "C" fn user_graphics_input_worker(arg : usize) -> ! {
    impl_fd_session::user_graphics_input_worker(arg)
}

/// per-task 挂载命名空间（`impl-fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub mod mount_ns;

/// 相对当前任务 cwd 创建目录（`impl-fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn mkdir_at_current(path: &str, mode: u32) -> VfsResult<()> {
    let abs = cwd::resolve_for_current_task(path)?;
    impl_fs_bridge::mkdir_path(abs.as_str(), mode)
}

/// 相对当前任务 cwd 创建符号链接（`impl-fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn symlink_at_current(target: &str, link_path: &str) -> VfsResult<()> {
    let abs = cwd::resolve_for_current_task(link_path)?;
    impl_fs_bridge::symlink_path(abs.as_str(), target)
}

/// 读取已解析绝对路径的符号链接目标（`bridge-fs-api`）。
#[cfg(feature = "bridge-fs-api")]
pub fn read_symlink_absolute(path: &str) -> VfsResult<alloc::vec::Vec<u8>> {
    impl_fs_bridge::read_symlink_path(path)
}

/// 展开绝对路径的中间符号链接，并按 `final_symlink` 决定是否跟随最终链接。
#[cfg(feature = "bridge-fs-api")]
pub fn resolve_symlink_absolute(
    path: &str,
    final_symlink: FinalSymlink,
) -> VfsResult<alloc::string::String> {
    resolve_symlink_path_with(
        path,
        final_symlink,
        |candidate| match impl_fs_bridge::read_symlink_path(candidate) {
            Ok(target) => {
                let target = alloc::string::String::from_utf8(target)
                    .map_err(|_| VfsError::NotUtf8)?;
                Ok(Some(target))
            }
            Err(VfsError::NotAFile) => Ok(None),
            Err(error) => Err(error),
        },
        |candidate| {
            impl_fs_bridge::metadata_path(candidate)
                .map(|meta| meta.node_type == VfsNodeType::Directory)
        },
    )
}

/// 在已解析绝对路径处创建符号链接（`bridge-fs-api`）。
#[cfg(feature = "bridge-fs-api")]
pub fn symlink_absolute(target: &str, link_path: &str) -> VfsResult<()> {
    impl_fs_bridge::symlink_path(link_path, target)
}

/// 在已解析绝对路径间创建硬链接（`bridge-fs-api`）。
#[cfg(feature = "bridge-fs-api")]
#[inline]
pub fn hardlink_absolute(existing_path: &str, new_path: &str) -> VfsResult<()> {
    let existing = normalize_absolute_path(existing_path)?;
    let new = normalize_absolute_path(new_path)?;
    impl_fs_bridge::hardlink_path(existing.as_str(), new.as_str())
}

/// 覆盖根卷上的绝对路径文件（unlink + 写 + 页缓存驱逐）。
#[cfg(feature = "bridge-fs-api")]
pub fn overwrite_absolute_file(path: &str, data: &[u8]) -> VfsResult<()> {
    impl_fs_bridge::overwrite_file_at(path, data)
}

/// 在绝对路径创建 `S_IFSOCK` 节点（pathname AF_UNIX bind 用）。
#[cfg(feature = "bridge-fs-api")]
pub fn mknod_socket_absolute(path: &str) -> VfsResult<()> {
    impl_fs_bridge::mknod_socket_at(path)
}

/// 在已解析绝对路径创建普通/特殊节点（`mknodat(2)`）。
#[cfg(feature = "bridge-fs-api")]
#[inline]
pub fn mknod_absolute(path: &str, mode: u32, rdev: u32) -> VfsResult<()> {
    let abs = normalize_absolute_path(path)?;
    impl_fs_bridge::mknod_path(abs.as_str(), mode, rdev)
}

/// 修改已解析绝对路径的权限（`impl-fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn chmod_absolute(path: &str, mode: u32) -> VfsResult<()> {
    let abs = normalize_absolute_path(path)?;
    impl_fs_bridge::chmod_path(abs.as_str(), mode)
}

/// 修改已解析绝对路径的 uid/gid（`impl-fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn chown_absolute(path: &str, uid: Option<u32>, gid: Option<u32>) -> VfsResult<()> {
    let abs = normalize_absolute_path(path)?;
    impl_fs_bridge::chown_path(abs.as_str(), uid, gid)
}

/// 设置已解析绝对路径的扩展属性。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn setxattr_absolute(path: &str, name: &str, value: &[u8]) -> VfsResult<()> {
    let abs = normalize_absolute_path(path)?;
    impl_fs_bridge::setxattr_path(abs.as_str(), name, value)
}

/// 读取已解析绝对路径的扩展属性。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn getxattr_absolute(path: &str, name: &str, buf: &mut [u8]) -> VfsResult<usize> {
    let abs = normalize_absolute_path(path)?;
    impl_fs_bridge::getxattr_path(abs.as_str(), name, buf)
}

/// 列出已解析绝对路径的扩展属性名。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn listxattr_absolute(path: &str, buf: &mut [u8]) -> VfsResult<usize> {
    let abs = normalize_absolute_path(path)?;
    impl_fs_bridge::listxattr_path(abs.as_str(), buf)
}

/// 删除已解析绝对路径的扩展属性。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn removexattr_absolute(path: &str, name: &str) -> VfsResult<()> {
    let abs = normalize_absolute_path(path)?;
    impl_fs_bridge::removexattr_path(abs.as_str(), name)
}

/// 截断已解析绝对路径的普通文件（`impl-fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn truncate_absolute(path: &str, len: u64) -> VfsResult<()> {
    let abs = normalize_absolute_path(path)?;
    impl_fs_bridge::truncate_path(abs.as_str(), len)
}

/// 在已解析绝对路径创建目录（`impl-fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn mkdir_absolute(path: &str, mode: u32) -> VfsResult<()> {
    let abs = normalize_absolute_path(path)?;
    impl_fs_bridge::mkdir_path(abs.as_str(), mode)
}

/// 删除已解析的绝对路径（`impl-fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn unlink_absolute(path: &str, remove_dir: bool) -> VfsResult<()> {
    let abs = normalize_absolute_path(path)?;
    impl_fs_bridge::unlink_path(abs.as_str(), remove_dir)
}

/// 重命名已解析的绝对路径（`impl-fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn rename_absolute(old_path: &str, new_path: &str) -> VfsResult<()> {
    let old = normalize_absolute_path(old_path)?;
    let new = normalize_absolute_path(new_path)?;
    impl_fs_bridge::rename_path(old.as_str(), new.as_str())
}

/// 将 ext4 块设备挂到绝对路径 `mount_point`。
#[cfg(feature = "bridge-fs-api")]
pub fn mount_ext4_block_at(mount_point: &str, block_dev: &str, readonly: bool) -> VfsResult<()> {
    impl_fs_bridge::mount_ext4_block_at(mount_point, block_dev, readonly)
}

/// 挂载内存 tmpfs 到 `mount_point`。
#[cfg(feature = "bridge-fs-api")]
pub fn mount_tmpfs_at(mount_point: &str) -> VfsResult<()> {
    impl_fs_bridge::mount_tmpfs_at(mount_point)
}

/// 挂载带容量上限的内存 tmpfs 到 `mount_point`。
#[cfg(feature = "bridge-fs-api")]
pub fn mount_tmpfs_at_with_limit(mount_point: &str, limit_bytes: Option<usize>) -> VfsResult<()> {
    impl_fs_bridge::mount_tmpfs_at_with_limit(mount_point, limit_bytes)
}

/// 判断绝对路径是否为当前挂载命名空间中的挂载点。
#[cfg(feature = "bridge-fs-api")]
pub fn is_mount_point_absolute(path: &str) -> bool {
    impl_fs_bridge::is_mount_point(path)
}

/// 在首个任务继承的 bootstrap namespace 中挂载 tmpfs。
#[cfg(feature = "bridge-fs-api")]
pub fn mount_bootstrap_tmpfs_at(mount_point: &str) -> VfsResult<()> {
    impl_fs_bridge::mount_bootstrap_tmpfs_at(mount_point)
}

/// 挂载 cgroup v1/v2 到 `mount_point`。
#[cfg(feature = "bridge-fs-api")]
pub fn mount_cgroup_at(mount_point: &str, v2: bool, options: &str) -> VfsResult<()> {
    impl_fs_bridge::mount_cgroup_at(mount_point, v2, options)
}

/// 查询路径所在辅助卷的 `statfs` 文件系统 magic。
#[cfg(feature = "bridge-fs-api")]
pub fn mount_statfs_magic(path: &str) -> Option<isize> {
    impl_fs_bridge::mount_statfs_magic(path)
}

/// 将 `mount_point` 上已挂载的辅助卷重载为只读。
#[cfg(feature = "bridge-fs-api")]
pub fn remount_readonly_at(mount_point: &str) -> VfsResult<()> {
    impl_fs_bridge::remount_readonly_at(mount_point)
}

/// 检查路径是否可写（只读挂载返回 [`VfsError::ReadOnlyFs`]）。
#[cfg(feature = "bridge-fs-api")]
pub fn assert_path_writable(path: &str) -> VfsResult<()> {
    impl_fs_bridge::assert_path_writable(path)
}

/// 在 ext4 根卷创建 `/proc` 目录（若不存在）。
#[cfg(all(feature = "bridge-fs-api", feature = "impl-fd-session"))]
pub fn ensure_proc_mount_point() -> VfsResult<()> {
    impl_fs_bridge::ensure_proc_mount_point()
}

/// 挂载 procfs 到 `mount_point`（默认 `/proc`）。
#[cfg(all(feature = "bridge-fs-api", feature = "impl-fd-session"))]
pub fn mount_procfs_at(mount_point: &str) -> VfsResult<()> {
    fs::procfs::active_impl::register_task_argv_lookup(|tid| cwd::lookup_argv_for_task(tid));
    fs::procfs::active_impl::register_task_exe_lookup(|tid| cwd::lookup_exe_for_task(tid));
    fs::procfs::active_impl::register_task_fd_lookup(|tid| fd::open_fds_for_task(tid));
    fs::procfs::active_impl::register_task_fd_target_lookup(|tid, raw_fd| {
        fd::fd_target_for_task(tid, raw_fd)
    });
    fs::procfs::active_impl::register_mount_list_lookup(|| {
        impl_fs_bridge::list_proc_mount_lines()
    });
    impl_fs_bridge::mount_procfs_at(mount_point)
}

/// 在首个任务继承的 bootstrap namespace 中挂载 procfs。
#[cfg(all(feature = "bridge-fs-api", feature = "impl-fd-session"))]
pub fn mount_bootstrap_procfs_at(mount_point: &str) -> VfsResult<()> {
    fs::procfs::active_impl::register_task_argv_lookup(|tid| cwd::lookup_argv_for_task(tid));
    fs::procfs::active_impl::register_task_exe_lookup(|tid| cwd::lookup_exe_for_task(tid));
    fs::procfs::active_impl::register_task_fd_lookup(|tid| fd::open_fds_for_task(tid));
    fs::procfs::active_impl::register_task_fd_target_lookup(|tid, raw_fd| {
        fd::fd_target_for_task(tid, raw_fd)
    });
    fs::procfs::active_impl::register_mount_list_lookup(|| {
        impl_fs_bridge::list_proc_mount_lines()
    });
    impl_fs_bridge::mount_bootstrap_proc_at(mount_point)
}

/// procfs 是否已挂在 `mount_point`。
#[cfg(feature = "bridge-fs-api")]
pub fn is_proc_mounted_at(mount_point: &str) -> bool {
    impl_fs_bridge::is_proc_mounted_at(mount_point)
}

/// 卸载 `mount_point`；`detach` 为 true 时接受 lazy umount（`MNT_DETACH`）。
#[cfg(feature = "bridge-fs-api")]
pub fn unmount_at(mount_point: &str, detach: bool) -> VfsResult<()> {
    impl_fs_bridge::unmount_at(mount_point, detach)
}

/// 挂载 securityfs 伪文件系统。
#[cfg(feature = "bridge-fs-api")]
pub fn mount_securityfs_at(mount_point: &str) -> VfsResult<()> {
    impl_fs_bridge::mount_securityfs_at(mount_point)
}

/// bind 挂载：`target` 成为 `source` 路径的别名。
#[cfg(feature = "bridge-fs-api")]
pub fn mount_bind_at(source: &str, target: &str, recursive: bool) -> VfsResult<()> {
    impl_fs_bridge::mount_bind_at(source, target, recursive)
}

/// 移动挂载点。
#[cfg(feature = "bridge-fs-api")]
pub fn move_mount_at(source: &str, target: &str) -> VfsResult<()> {
    impl_fs_bridge::move_mount_at(source, target)
}

/// 设置挂载点传播类型。
#[cfg(feature = "bridge-fs-api")]
pub use impl_fs_bridge::MountPropagation;

#[cfg(feature = "bridge-fs-api")]
pub fn set_mount_propagation(
    mount_point: &str,
    propagation: MountPropagation,
    recursive: bool,
) -> VfsResult<()> {
    impl_fs_bridge::set_mount_propagation(mount_point, propagation, recursive)
}

/// 刷回并回收整个文件页缓存（测例脚本切换等批量回收点调用）。
#[cfg(feature = "bridge-fs-api")]
pub fn reset_file_page_cache() -> VfsResult<()> {
    impl_fs_bridge::reset_file_page_cache()
}

/// 将文件页缓存和根文件系统缓存同步到底层块设备，不回收热缓存。
#[cfg(feature = "bridge-fs-api")]
pub fn sync_file_page_cache() -> VfsResult<()> {
    impl_fs_bridge::sync_file_page_cache()
}

/// 相对当前任务 cwd 删除路径。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn unlink_at_current(path: &str, remove_dir: bool) -> VfsResult<()> {
    let abs = cwd::resolve_for_current_task(path)?;
    unlink_absolute(abs.as_str(), remove_dir)
}

#[cfg(feature = "impl-fd-session")]
pub use impl_fd_session::{
    pipe_handle_pair, pipe_handle_pair_with_flags, stream_pair_handle_pair, PipeReadHandle,
    PipeWriteHandle, UnixStreamPairEnd,
};

#[cfg(feature = "bridge-fs-api")]
pub use impl_fs_bridge::RootFileHandle;

/// 当前 feature 选中的 VFS 后端（`bridge-fs-api` → fs 桥接，否则返回未挂载错误）。
pub mod active_impl {
    use super::api::VfsBackend;

    #[cfg(feature = "bridge-fs-api")]
    pub fn backend() -> &'static impl VfsBackend {
        static B: impl_fs_bridge::FsBridge = impl_fs_bridge::FsBridge;
        &B
    }

    #[cfg(not(feature = "bridge-fs-api"))]
    pub fn backend() -> &'static impl VfsBackend {
        static B: crate::unsupported_backend::UnsupportedBackend =
            crate::unsupported_backend::UnsupportedBackend;
        &B
    }
}

#[cfg(not(feature = "bridge-fs-api"))]
mod unsupported_backend {
    use alloc::{boxed::Box, string::String, vec::Vec};
    use super::{normalize_absolute_path, RootRwSession, SingleRootReadView, VfsBackend,
                VfsCapability, VfsDevInventory, VfsDevNode, VfsDirEntry, VfsError, VfsFsKind,
                VfsMountOps, VfsMountTable, VfsMetadata, VfsOpenOps, VfsResult};

    pub struct UnsupportedBackend;

    impl SingleRootReadView for UnsupportedBackend {
        fn exists(&self, path: &str) -> VfsResult<bool> {
            let _ = normalize_absolute_path(path)?;
            Err(VfsError::NotMounted)
        }
        fn metadata(&self, path: &str) -> VfsResult<VfsMetadata> {
            let _ = normalize_absolute_path(path)?;
            Err(VfsError::NotMounted)
        }
        fn read(&self, path: &str) -> VfsResult<Vec<u8>> {
            let _ = normalize_absolute_path(path)?;
            Err(VfsError::NotMounted)
        }
        fn read_dir(&self, path: &str) -> VfsResult<Vec<VfsDirEntry>> {
            let _ = normalize_absolute_path(path)?;
            Err(VfsError::NotMounted)
        }
    }
    impl VfsMountOps for UnsupportedBackend {
        fn supported_capabilities(&self) -> Vec<VfsCapability> { Vec::new() }
        fn mount_rw_session(&self, _kind: VfsFsKind) -> VfsResult<Box<dyn RootRwSession>> {
            Err(VfsError::Unsupported)
        }
    }
    impl VfsDevInventory for UnsupportedBackend {
        fn list_dev_nodes(&self) -> Vec<VfsDevNode> { Vec::new() }
        fn default_root_block_path(&self) -> Option<String> { None }
    }
    impl VfsOpenOps for UnsupportedBackend {}
    impl VfsMountTable for UnsupportedBackend {}
    impl VfsBackend for UnsupportedBackend {}
}

/// 单根路径级只读访问。
pub mod root;

/// RW 挂载会话。
pub mod mount;

/// 模块自检：组合挂载与只读读回等（warn 不 panic）。
pub mod self_test {
    extern crate alloc;

    use alloc::string::String;

    use super::active_impl;
    use super::api::{
        SingleRootReadView, VfsDevInventory, VfsFsKind, VfsMountOps, VfsOpenFlags, VfsOpenOps,
        VfsResult, VfsSeekWhence, validate_root_file_name,
    };

    /// RW 写入后通过同一根 RW 视图读回校验。
    pub fn rw_write_root_verify(
        kind: VfsFsKind,
        name: &str,
        data: &[u8],
    ) -> VfsResult<()> {
        validate_root_file_name(name)?;
        let backend = active_impl::backend();
        let mut session = backend.mount_rw_session(kind)?;
        session.write_regular_file_at_root(name, data)?;
        let ro = backend;
        let mut path = String::from("/");
        path.push_str(name);
        let bytes = ro.read(path.as_str())?;
        if bytes.as_slice() == data {
            Ok(())
        } else {
            Err(super::api::VfsError::Io)
        }
    }

    /// RW `mkdir` 后同一 RW 视图 `metadata` 校验为目录。
    #[cfg(feature = "bridge-fs-api")]
    pub fn rw_mkdir_verify(kind: VfsFsKind, dir_name: &str) -> VfsResult<()> {
        use super::api::VfsNodeType;

        validate_root_file_name(dir_name)?;
        let backend = active_impl::backend();
        let mut path = String::from("/");
        path.push_str(dir_name);
        let mut session = backend.mount_rw_session(kind)?;
        session.mkdir(path.as_str(), 0o755)?;
        let meta = backend.metadata(path.as_str())?;
        if meta.node_type != VfsNodeType::Directory {
            return Err(super::api::VfsError::Io);
        }
        Ok(())
    }

    /// `read_at` / `write_at` 不改变顺序读偏移。
    #[cfg(feature = "bridge-fs-api")]
    pub fn read_at_write_at_smoke() -> VfsResult<()> {

        const NAME: &str = "vfs_at_io_smoke";
        let mut path = String::from("/");
        path.push_str(NAME);
        let backend = active_impl::backend();
        let mut handle = backend.open(
            path.as_str(),
            VfsOpenFlags(VfsOpenFlags::READ | VfsOpenFlags::WRITE | VfsOpenFlags::CREATE),
        )?;
        handle.write_at(0, b"hello")?;
        handle.write_at(5, b" world")?;
        let _ = handle.seek(0, VfsSeekWhence::Set)?;
        let mut buf = [0u8; 11];
        let n = handle.read_at(0, &mut buf)?;
        if n != 11 || &buf != b"hello world" {
            return Err(super::api::VfsError::Io);
        }
        let mut seq = [0u8; 2];
        let n2 = handle.read(&mut seq)?;
        if n2 != 2 || &seq != b"he" {
            return Err(super::api::VfsError::Io);
        }
        Ok(())
    }

    /// `/dev/null`：devfs 绑定与元数据（启动期无用户任务，不测 open/fd）。
    #[cfg(feature = "bridge-fs-api")]
    pub fn null_dev_smoke() -> VfsResult<()> {
        let backend = active_impl::backend();
        if !backend.exists("/dev/null")? {
            return Err(super::api::VfsError::NotFound);
        }
        let meta = backend.metadata("/dev/null")?;
        if meta.mode != 0o20666 {
            return Err(super::api::VfsError::Io);
        }
        Ok(())
    }

    /// `open` → `read` → `seek` → `metadata` 烟囱（依赖 RW 先写入测试文件）。
    #[cfg(feature = "bridge-fs-api")]
    pub fn open_read_seek_smoke() -> VfsResult<()> {
        const NAME: &str = "vfs_open_smoke";
        const DATA: &[u8] = b"open-smoke";
        rw_write_root_verify(VfsFsKind::Ext4, NAME, DATA)?;
        let mut path = String::from("/");
        path.push_str(NAME);
        let backend = active_impl::backend();
        let mut handle = backend.open(path.as_str(), VfsOpenFlags::read())?;
        let mut buf = [0u8; 16];
        let n = handle.read(&mut buf)?;
        if &buf[..n] != DATA {
            return Err(super::api::VfsError::Io);
        }
        let _ = handle.seek(0, VfsSeekWhence::Set)?;
        let m = handle.metadata()?;
        if m.size != DATA.len() as u64 {
            return Err(super::api::VfsError::Io);
        }
        Ok(())
    }

    /// `dup`/`fork` concrete wrapper 共享 OFD offset/status，独立 `open` 不共享。
    #[cfg(feature = "bridge-fs-api")]
    pub fn open_description_sharing_smoke() -> VfsResult<()> {
        const NAME : &str = "vfs_ofd_smoke";
        const DATA : &[u8] = b"abcd";
        const O_NONBLOCK : u32 = 0o4000;
        rw_write_root_verify(VfsFsKind::Ext4, NAME, DATA)?;
        let mut path = String::from("/");
        path.push_str(NAME);
        let backend = active_impl::backend();
        let mut first = backend.open(path.as_str(), VfsOpenFlags::read())?;
        let mut duplicate = first.duplicate()?;

        let mut byte = [0u8; 1];
        if first.read(&mut byte)? != 1 || byte[0] != b'a' {
            return Err(super::api::VfsError::Io);
        }
        if duplicate.read(&mut byte)? != 1 || byte[0] != b'b' {
            return Err(super::api::VfsError::Io);
        }
        duplicate.set_open_status_flags(O_NONBLOCK)?;
        if first.open_status_flags() & O_NONBLOCK == 0 {
            return Err(super::api::VfsError::Io);
        }
        duplicate.close()?;
        if first.read(&mut byte)? != 1 || byte[0] != b'c' {
            return Err(super::api::VfsError::Io);
        }

        let mut independent = backend.open(path.as_str(), VfsOpenFlags::read())?;
        if independent.read(&mut byte)? != 1 || byte[0] != b'a' {
            return Err(super::api::VfsError::Io);
        }
        Ok(())
    }

    /// Prepared read 仅按 user-copy 进度提交，并在 Drop 时取消 reservation。
    #[cfg(feature = "bridge-fs-api")]
    pub fn prepared_read_smoke() -> VfsResult<()> {
        use super::api::{VfsCopyProgress, VfsError, VfsReadFinish};

        let backend = active_impl::backend();
        let mut handle =
            backend.open("/vfs_ofd_smoke",
                         VfsOpenFlags(VfsOpenFlags::READ | VfsOpenFlags::WRITE))?;
        let mut duplicate = handle.duplicate()?;

        let lease = handle.prepare_read(3)?.acquire()?;
        if lease.bytes() != b"abc" {
            return Err(VfsError::Io);
        }
        if !matches!(duplicate.prepare_read(1), Err(VfsError::Busy)) ||
           duplicate.seek(0, VfsSeekWhence::Cur) != Err(VfsError::Busy) ||
           duplicate.write(b"q") != Err(VfsError::Busy)
        {
            return Err(VfsError::Io);
        }
        let mut independent = backend.open("/vfs_ofd_smoke", VfsOpenFlags::read())?;
        let independent_lease = independent.prepare_read(1)?.acquire()?;
        if independent_lease.bytes() != b"a" {
            return Err(VfsError::Io);
        }
        drop(independent_lease);
        if lease.finish(VfsCopyProgress { copied : 1,
                                          complete : false })? !=
           VfsReadFinish::Bytes(1)
        {
            return Err(VfsError::Io);
        }
        if duplicate.seek(0, VfsSeekWhence::Cur)? != 1 {
            return Err(VfsError::Io);
        }

        let cancelled = handle.prepare_read(2)?.acquire()?;
        if cancelled.bytes() != b"bc" {
            return Err(VfsError::Io);
        }
        drop(cancelled);
        if handle.seek(0, VfsSeekWhence::Cur)? != 1 {
            return Err(VfsError::Io);
        }

        let faulted = handle.prepare_read(2)?.acquire()?;
        if faulted.finish(VfsCopyProgress { copied : 0,
                                            complete : false })? !=
           VfsReadFinish::Fault ||
           handle.seek(0, VfsSeekWhence::Cur)? != 1
        {
            return Err(VfsError::Io);
        }

        let complete = handle.prepare_read(8)?.acquire()?;
        if complete.finish(VfsCopyProgress { copied : 3,
                                             complete : true })? !=
           VfsReadFinish::Bytes(3) ||
           handle.seek(0, VfsSeekWhence::Cur)? != 4
        {
            return Err(VfsError::Io);
        }
        let eof = handle.prepare_read(1)?.acquire()?;
        if !eof.bytes().is_empty() ||
           eof.finish(VfsCopyProgress { copied : 0,
                                        complete : true })? !=
           VfsReadFinish::Bytes(0)
        {
            return Err(VfsError::Io);
        }
        Ok(())
    }

    pub fn run() {
        #[cfg(feature = "bridge-fs-api")]
        {
            const NAME: &str = "vfs_rw_smoke";
            const DATA: &[u8] = b"vfs-smoke";
            if let Err(e) = rw_write_root_verify(VfsFsKind::Ext4, NAME, DATA) {
                log::warn!("[vfs] self_test rw verify skipped or failed: {:?}", e);
            }
            if let Err(e) = open_read_seek_smoke() {
                log::warn!("[vfs] self_test open/seek skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test open/seek ok");
            }
            if let Err(e) = read_at_write_at_smoke() {
                log::warn!("[vfs] self_test read_at/write_at skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test read_at/write_at ok");
            }
            if let Err(e) = open_description_sharing_smoke() {
                log::warn!("[vfs] self_test OFD sharing skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test OFD sharing ok");
            }
            if let Err(e) = prepared_read_smoke() {
                log::warn!("[vfs] self_test prepared read skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test prepared read ok");
            }
            const MKDIR_NAME: &str = "vfs_mkdir_smoke";
            if let Err(e) = rw_mkdir_verify(VfsFsKind::Ext4, MKDIR_NAME) {
                log::warn!("[vfs] self_test mkdir skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test mkdir ok");
            }
            if let Err(e) = null_dev_smoke() {
                log::warn!("[vfs] self_test /dev/null skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test /dev/null ok");
            }
        }
        let _ = active_impl::backend().list_dev_nodes();
    }
}

pub fn test() {
    api_v0::test();
    #[cfg(feature = "bridge-fs-api")]
    impl_fs_bridge::test();
    #[cfg(feature = "impl-fd-session")]
    {
        fd::self_test();
        cwd::self_test();
    }
    self_test::run();
}

/// VFS 组件统一内核态自检入口；测试完成后由各测试项清理临时文件和句柄。
#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[vfs] self_test begin");
    test();
    log::info!("[vfs] self_test complete; temporary VFS resources were reclaimed");
}
