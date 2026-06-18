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
    validate_root_file_name, NormalizedPath,
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

/// 相对当前任务 cwd 创建目录（`impl-fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn mkdir_at_current(path: &str, mode: u32) -> VfsResult<()> {
    let abs = cwd::resolve_for_current_task(path)?;
    impl_fs_bridge::mkdir_path(abs.as_str(), mode)
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
    fs::procfs::active_impl::register_mount_list_lookup(|| {
        impl_fs_bridge::list_proc_mount_lines()
    });
    impl_fs_bridge::mount_procfs_at(mount_point)
}

/// procfs 是否已挂在 `mount_point`。
#[cfg(feature = "bridge-fs-api")]
pub fn is_proc_mounted_at(mount_point: &str) -> bool {
    impl_fs_bridge::is_proc_mounted_at(mount_point)
}

/// 卸载 `mount_point`。
#[cfg(feature = "bridge-fs-api")]
pub fn unmount_at(mount_point: &str) -> VfsResult<()> {
    impl_fs_bridge::unmount_at(mount_point)
}

/// 相对当前任务 cwd 删除路径。
#[cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]
pub fn unlink_at_current(path: &str, remove_dir: bool) -> VfsResult<()> {
    let abs = cwd::resolve_for_current_task(path)?;
    unlink_absolute(abs.as_str(), remove_dir)
}

#[cfg(feature = "impl-fd-session")]
pub use impl_fd_session::{
    pipe_handle_pair, stream_pair_handle_pair, PipeReadHandle, PipeWriteHandle,
    UnixStreamPairEnd,
};

#[cfg(feature = "bridge-fs-api")]
pub use impl_fs_bridge::RootFileHandle;

/// 当前 feature 选中的 VFS 后端（`bridge-fs-api` → fs 桥接，否则占位）。
pub mod active_impl {
    use super::api::VfsBackend;

    #[cfg(feature = "bridge-fs-api")]
    pub fn backend() -> &'static impl VfsBackend {
        static B: impl_fs_bridge::FsBridge = impl_fs_bridge::FsBridge;
        &B
    }

    #[cfg(not(feature = "bridge-fs-api"))]
    pub fn backend() -> &'static impl VfsBackend {
        static B: crate::impl_dummy::DummyBackend = crate::impl_dummy::DummyBackend;
        &B
    }
}

/// 单根路径级只读访问。
pub mod root {
    use super::active_impl;
    use super::api::SingleRootReadView;

    pub fn read_view() -> &'static impl SingleRootReadView {
        active_impl::backend()
    }
}

/// RW 挂载会话。
pub mod mount {
    use alloc::boxed::Box;

    use super::active_impl;
    use super::api::{RootRwSession, VfsFsKind, VfsMountOps, VfsResult};

    pub fn open_rw_session(kind: VfsFsKind) -> VfsResult<Box<dyn RootRwSession>> {
        active_impl::backend().mount_rw_session(kind)
    }

    pub fn supported_capabilities() -> alloc::vec::Vec<super::api::VfsCapability> {
        active_impl::backend().supported_capabilities()
    }
}

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
        use super::api::VfsIoHandle;

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

#[doc(hidden)]
pub mod dummy {
    pub use ::impl_dummy::*;
}

pub fn test() {
    api_v0::test();
    impl_dummy::test();
    #[cfg(feature = "bridge-fs-api")]
    impl_fs_bridge::test();
    #[cfg(feature = "impl-fd-session")]
    {
        fd::self_test();
        cwd::self_test();
    }
    self_test::run();
}
