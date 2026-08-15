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
pub use api_v0::*;

/// `/proc/<pid>/ns/<type>` 是 Linux 的 magic link：`readlink` 返回
/// `type:[inode]`，但路径解析不能把该文本当作普通文件名继续跟随。
fn is_proc_namespace_magic_link(path : &str, target : &str) -> bool {
    let mut components = path.rsplit('/').filter(|part| !part.is_empty());
    let Some(namespace) = components.next() else { return false; };
    if components.next() != Some("ns") || components.next().is_none() ||
       components.next() != Some("proc")
    {
        return false;
    }
    let Some(inode) = target.strip_prefix(namespace)
                            .and_then(|rest| rest.strip_prefix(":["))
                            .and_then(|rest| rest.strip_suffix(']'))
    else {
        return false;
    };
    !inode.is_empty() && inode.bytes().all(|byte| byte.is_ascii_digit())
}

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

/// 在指定目录所属的可写挂载上创建 Linux `O_TMPFILE` 匿名文件句柄。
#[cfg(feature = "bridge-fs-api")]
pub fn open_tmpfile_absolute(
    directory: &str,
    flags: VfsOpenFlags,
    mode: u32,
    uid: u32,
    gid: u32,
    linkable: bool,
) -> VfsResult<alloc::boxed::Box<dyn VfsIoHandle>> {
    impl_fs_bridge::open_tmpfile_path(directory, flags, mode, uid, gid, linkable)
}

/// 展开绝对路径的中间符号链接，并按 `final_symlink` 决定是否跟随最终链接。
#[cfg(feature = "bridge-fs-api")]
pub fn resolve_symlink_absolute(
    path: &str,
    final_symlink: FinalSymlink,
) -> VfsResult<alloc::string::String> {
    #[cfg(feature = "impl-fd-session")]
    if let Ok(root) = cwd::current_root() {
        let physical = if root != "/" && path != root &&
                          !(path.starts_with(root.as_str()) &&
                            path.as_bytes().get(root.len()) == Some(&b'/'))
        {
            cwd::resolve_for_current_task(path)?
        } else {
            alloc::string::String::from(path)
        };
        return resolve_symlink_in_root_absolute(physical.as_str(), root.as_str(), final_symlink);
    }
    resolve_symlink_path_with(
        path,
        final_symlink,
        |candidate| match impl_fs_bridge::read_symlink_path(candidate) {
            Ok(target) => {
                let target = alloc::string::String::from_utf8(target)
                    .map_err(|_| VfsError::NotUtf8)?;
                if is_proc_namespace_magic_link(candidate, target.as_str()) {
                    Ok(None)
                } else {
                    Ok(Some(target))
                }
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

#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn resolve_symlink_in_root_absolute(
    path: &str,
    root: &str,
    final_symlink: FinalSymlink,
) -> VfsResult<alloc::string::String> {
    use alloc::{string::String, vec::Vec};
    const MAX_SYMLINKS: usize = 40;

    let logical = if root == "/" {
        path
    } else if path == root {
        "/"
    } else {
        path.strip_prefix(root)
            .filter(|suffix| suffix.starts_with('/'))
            .ok_or(VfsError::AccessDenied)?
    };
    let mut pending: Vec<String> = logical.split('/')
                                             .filter(|part| !part.is_empty())
                                             .map(String::from)
                                             .collect();
    let mut resolved = String::from(root);
    let mut followed = 0usize;
    while !pending.is_empty() {
        let component = pending.remove(0);
        let candidate = if resolved == "/" {
            alloc::format!("/{component}")
        } else {
            alloc::format!("{}/{component}", resolved.trim_end_matches('/'))
        };
        let is_final = pending.is_empty();
        if !is_final || final_symlink == FinalSymlink::Follow {
            match impl_fs_bridge::read_symlink_path(candidate.as_str()) {
                Ok(target) => {
                    let target = String::from_utf8(target).map_err(|_| VfsError::NotUtf8)?;
                    if is_proc_namespace_magic_link(candidate.as_str(), target.as_str()) {
                        resolved = candidate;
                        continue;
                    }
                    if followed == MAX_SYMLINKS {
                        return Err(VfsError::TooManySymlinks);
                    }
                    followed += 1;
                    let mut combined = cwd::resolve_with_virtual_root(root,
                                                                      resolved.as_str(),
                                                                      target.as_str())?;
                    if !pending.is_empty() {
                        combined = cwd::resolve_with_virtual_root(root,
                                                                  combined.as_str(),
                                                                  pending.join("/").as_str())?;
                    }
                    let suffix = if root == "/" {
                        combined.as_str()
                    } else if combined == root {
                        "/"
                    } else {
                        combined.strip_prefix(root).ok_or(VfsError::AccessDenied)?
                    };
                    pending = suffix.split('/')
                                    .filter(|part| !part.is_empty())
                                    .map(String::from)
                                    .collect();
                    resolved = String::from(root);
                    continue;
                }
                Err(VfsError::NotAFile) => {}
                Err(error) => return Err(error),
            }
        }
        if !is_final && impl_fs_bridge::metadata_path(candidate.as_str())?.node_type !=
                        VfsNodeType::Directory
        {
            return Err(VfsError::NotDirectory);
        }
        resolved = candidate;
    }
    Ok(resolved)
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

/// 在根卷创建 `/sys` 挂载点（若不存在）。
#[cfg(all(feature = "bridge-fs-api", feature = "impl-fd-session"))]
pub fn ensure_sys_mount_point() -> VfsResult<()> {
    impl_fs_bridge::ensure_sys_mount_point()
}

/// 挂载 procfs 到 `mount_point`（默认 `/proc`）。
#[cfg(all(feature = "bridge-fs-api", feature = "impl-fd-session"))]
pub fn mount_procfs_at(mount_point: &str) -> VfsResult<()> {
    fs::procfs::active_impl::register_task_argv_lookup(|tid| cwd::lookup_argv_for_task(tid));
    fs::procfs::active_impl::register_task_env_lookup(|tid| cwd::lookup_env_for_task(tid));
    fs::procfs::active_impl::register_task_auxv_lookup(|tid| cwd::lookup_auxv_for_task(tid));
    fs::procfs::active_impl::register_task_exe_lookup(|tid| cwd::lookup_exe_for_task(tid));
    fs::procfs::active_impl::register_task_cwd_lookup(|tid| cwd::lookup_cwd_for_task(tid));
    fs::procfs::active_impl::register_task_root_lookup(|tid| cwd::lookup_root_for_task(tid));
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
    fs::procfs::active_impl::register_task_env_lookup(|tid| cwd::lookup_env_for_task(tid));
    fs::procfs::active_impl::register_task_auxv_lookup(|tid| cwd::lookup_auxv_for_task(tid));
    fs::procfs::active_impl::register_task_exe_lookup(|tid| cwd::lookup_exe_for_task(tid));
    fs::procfs::active_impl::register_task_cwd_lookup(|tid| cwd::lookup_cwd_for_task(tid));
    fs::procfs::active_impl::register_task_root_lookup(|tid| cwd::lookup_root_for_task(tid));
    fs::procfs::active_impl::register_task_fd_lookup(|tid| fd::open_fds_for_task(tid));
    fs::procfs::active_impl::register_task_fd_target_lookup(|tid, raw_fd| {
        fd::fd_target_for_task(tid, raw_fd)
    });
    fs::procfs::active_impl::register_mount_list_lookup(|| {
        impl_fs_bridge::list_proc_mount_lines()
    });
    impl_fs_bridge::mount_bootstrap_proc_at(mount_point)
}

/// 挂载 sysfs 到当前任务的挂载命名空间。
#[cfg(all(feature = "bridge-fs-api", feature = "impl-fd-session"))]
pub fn mount_sysfs_at(mount_point: &str) -> VfsResult<()> {
    impl_fs_bridge::mount_sysfs_at(mount_point)
}

/// 在 bootstrap namespace 中挂载 sysfs，供之后创建的任务继承。
#[cfg(all(feature = "bridge-fs-api", feature = "impl-fd-session"))]
pub fn mount_bootstrap_sysfs_at(mount_point: &str) -> VfsResult<()> {
    impl_fs_bridge::mount_bootstrap_sys_at(mount_point)
}

/// procfs 是否已挂在 `mount_point`。
#[cfg(feature = "bridge-fs-api")]
pub fn is_proc_mounted_at(mount_point: &str) -> bool {
    impl_fs_bridge::is_proc_mounted_at(mount_point)
}

/// 查询路径是否为当前挂载命名空间中的挂载点。
#[cfg(feature = "bridge-fs-api")]
pub fn is_mount_point(mount_point: &str) -> bool {
    impl_fs_bridge::is_mount_point(mount_point)
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
pub mod self_test;

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
    api_v0::test();
    #[cfg(feature = "bridge-fs-api")]
    impl_fs_bridge::self_test();
    #[cfg(feature = "impl-fd-session")]
    impl_fd_session::self_test();
    self_test::run();
    log::info!("[vfs] self_test complete; temporary VFS resources were reclaimed");
}
