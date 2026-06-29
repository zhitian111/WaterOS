//! 挂载与后端能力查询。

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};

use crate::error::VfsResult;
use crate::kind::{VfsCapability, VfsFsKind};
use crate::rw_session::RootRwSession;

/// 挂载相关操作：能力枚举与 RW 会话创建。
pub trait VfsMountOps {
    /// 当前后端声明支持的 `(FsKind, AccessMode)` 组合。
    fn supported_capabilities(&self) -> Vec<VfsCapability>;

    /// 打开指定种类的根级 RW 会话；不支持时返回 [`VfsError::Unsupported`]。
    fn mount_rw_session(&self, kind: VfsFsKind) -> VfsResult<Box<dyn RootRwSession>>;
}
