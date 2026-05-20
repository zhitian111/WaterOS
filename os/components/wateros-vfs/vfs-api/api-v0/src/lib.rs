//! VFS **公共 API v0**：定义整个虚拟文件系统模块的基本能力契约。
//!
//! `vfs-impl-*` 实现这些 trait；[`wateros-vfs`](../../src/lib.rs) 聚合层将能力组合为
//! `root`、`mount`、`self_test` 等对外接口。本 crate **不** 依赖 `wateros-fs`。

#![no_std]

extern crate alloc;

pub mod backend;
pub mod dev;
pub mod error;
pub mod fd;
pub mod handle;
pub mod kind;
pub mod meta;
pub mod mount;
pub mod namespace;
pub mod path;
pub mod resolve;
pub mod root_read;
pub mod rw_session;

pub use backend::VfsBackend;
pub use dev::{VfsDevInventory, VfsDevNode, VfsDevNodeType};
pub use error::{VfsError, VfsResult};
pub use fd::{
    VfsFd, VfsFdSession, VFS_FIRST_DYNAMIC_FD, VFS_STDERR_FD, VFS_STDIN_FD, VFS_STDOUT_FD,
};
pub use handle::{VfsFileHandle, VfsIoHandle, VfsOpenFlags, VfsOpenOps, VfsSeekWhence};
pub use kind::{VfsAccessMode, VfsCapability, VfsFsKind};
pub use meta::{VfsDirEntry, VfsMetadata, VfsNodeType};
pub use mount::VfsMountOps;
pub use namespace::VfsMountTable;
pub use path::{normalize_absolute_path, validate_root_file_name, NormalizedPath};
pub use resolve::{register_open_path_resolver, resolve_against_cwd, resolve_open_path};
pub use root_read::SingleRootReadView;
pub use rw_session::RootRwSession;

/// `api-v0` 内建单元测试：路径规范化与校验的固定样例。
pub fn test() {
    assert_eq!(
        normalize_absolute_path("/a/./b/../c").unwrap().as_str(),
        "/a/c"
    );
    assert_eq!(normalize_absolute_path("//").unwrap().as_str(), "/");
    assert!(validate_root_file_name("foo").is_ok());
    assert!(matches!(
        validate_root_file_name("a/b"),
        Err(VfsError::InvalidPath)
    ));
}
