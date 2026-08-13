//! 根文件系统读写挂载 facade。

use alloc::boxed::Box;

use super::active_impl;
use super::api::{RootRwSession, VfsCapability, VfsFsKind, VfsMountOps, VfsResult};

/// 为指定文件系统类型打开根读写会话。
pub fn open_rw_session(kind: VfsFsKind) -> VfsResult<Box<dyn RootRwSession>> {
    active_impl::backend().mount_rw_session(kind)
}

/// 返回当前后端支持的挂载能力。
pub fn supported_capabilities() -> alloc::vec::Vec<VfsCapability> {
    active_impl::backend().supported_capabilities()
}
