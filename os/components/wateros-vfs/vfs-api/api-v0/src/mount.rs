//! 挂载与后端能力查询。

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};

use crate::error::VfsResult;
use crate::kind::{VfsCapability, VfsFsKind};
use crate::rw_session::RootRwSession;

/// 挂载相关操作：能力枚举与 RW 会话创建。
pub trait VfsMountOps {
    fn supported_capabilities(&self) -> Vec<VfsCapability>;

    fn mount_rw_session(&self, kind: VfsFsKind) -> VfsResult<Box<dyn RootRwSession>>;
}
