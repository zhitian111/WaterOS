//! 虚拟文件系统 **聚合 crate**：[`api`] 定义基本能力，[`active_impl`] 选择后端，
//! [`root`] / [`mount`] / [`self_test`] 将能力组合为对外稳定接口。

#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;

#[cfg(feature = "bridge-fs-api")]
extern crate impl_fs_bridge;

#[cfg(feature = "fd-session")]
extern crate base;
#[cfg(feature = "fd-session")]
extern crate impl_fd_session;
#[cfg(feature = "fd-session")]
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

/// per-task 文件描述符会话（`fd-session` feature）。
#[cfg(feature = "fd-session")]
pub mod fd;

/// per-task 工作目录（`fd-session` feature）。
#[cfg(feature = "fd-session")]
pub mod cwd;

/// 相对当前任务 cwd 创建目录（`fd-session` + `bridge-fs-api`）。
#[cfg(all(feature = "fd-session", feature = "bridge-fs-api"))]
pub fn mkdir_at_current(path: &str, mode: u32) -> VfsResult<()> {
    let abs = cwd::resolve_for_current_task(path)?;
    let mut sess = mount::open_rw_session(VfsFsKind::Ext4)?;
    sess.mkdir(abs.as_str(), mode)
}

#[cfg(feature = "fd-session")]
pub use impl_fd_session::{PipeReadHandle, PipeWriteHandle};

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
            const MKDIR_NAME: &str = "vfs_mkdir_smoke";
            if let Err(e) = rw_mkdir_verify(VfsFsKind::Ext4, MKDIR_NAME) {
                log::warn!("[vfs] self_test mkdir skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test mkdir ok");
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
    #[cfg(feature = "fd-session")]
    {
        fd::self_test();
        cwd::self_test();
    }
    self_test::run();
}
