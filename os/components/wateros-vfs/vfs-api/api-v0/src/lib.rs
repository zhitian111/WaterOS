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
pub use handle::{
    VfsCopyProgress, VfsFileContentIdentity, VfsFileHandle, VfsIoHandle, VfsOpenDescriptionState,
    VfsOpenFlags, VfsOpenOps, VfsPreparedRead, VfsReadFinish, VfsReadLease, VfsReadReservation,
    VfsSeekWhence,
};
pub use kind::{VfsAccessMode, VfsCapability, VfsFsKind};
pub use meta::{VfsDirEntry, VfsMetadata, VfsNodeType};
pub use mount::VfsMountOps;
pub use namespace::VfsMountTable;
pub use path::{normalize_absolute_path, validate_root_file_name, NormalizedPath};
pub use resolve::{
    register_open_path_resolver, resolve_against_cwd, resolve_open_path,
    resolve_symlink_path_with, FinalSymlink,
};
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

    let links = [
        ("/lib", "usr/lib"),
        ("/usr/lib/tool", "../bin/real-tool"),
    ];
    let resolved = resolve_symlink_path_with(
        "/lib/tool",
        FinalSymlink::Follow,
        |path| {
            Ok(links
                .iter()
                .find(|(link, _)| *link == path)
                .map(|(_, target)| alloc::string::String::from(*target)))
        },
        |path| Ok(matches!(path, "/usr" | "/usr/lib" | "/usr/bin")),
    )
    .unwrap();
    assert_eq!(resolved, "/usr/bin/real-tool");

    let unresolved = resolve_symlink_path_with(
        "/lib",
        FinalSymlink::NoFollow,
        |_| Ok(None),
        |_| Ok(false),
    )
    .unwrap();
    assert_eq!(unresolved, "/lib");

    let looped = resolve_symlink_path_with(
        "/loop",
        FinalSymlink::Follow,
        |path| Ok((path == "/loop").then(|| alloc::string::String::from("/loop"))),
        |_| Ok(true),
    );
    assert_eq!(looped, Err(VfsError::TooManySymlinks));

    let description = alloc::sync::Arc::new(VfsOpenDescriptionState::new(4, 0));
    let duplicate = description.clone();
    assert_eq!(description.advance_offset(3), Ok(7));
    assert_eq!(duplicate.offset(), 7);
    duplicate.set_status_flags(0o4000);
    assert_eq!(description.status_flags(), 0o4000);
    assert_eq!(description.add_signed_offset(-2), Ok(5));
    assert_eq!(duplicate.next_reservation_generation(), 1);
    assert_eq!(description.next_reservation_generation(), 2);
    let reservation = description.begin_read().expect("reserve");
    assert_eq!(reservation.offset(), 5);
    assert_eq!(description.begin_read(), Err(VfsError::Busy));
    assert_eq!(description.finish_read(reservation, 2, 3), Ok(7));
    let cancelled = description.begin_read().expect("reserve cancel");
    assert_eq!(description.cancel_read(cancelled), Ok(()));
    assert_eq!(description.offset(), 7);
}

#[cfg(test)]
mod tests {
    #[test]
    fn path_contracts() {
        super::test();
    }
}
