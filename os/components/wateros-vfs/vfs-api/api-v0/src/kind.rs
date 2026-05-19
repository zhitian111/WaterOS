//! 后端文件系统种类与能力声明（VFS 自有类型，不依赖 `wateros-fs`）。

/// 文件系统类型标识（VFS 命名空间）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VfsFsKind {
    Ext2,
    Ext3,
    Ext4,
    /// 其他具名子系统。
    Other(&'static str),
}

/// 挂载访问模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VfsAccessMode {
    ReadOnly,
    ReadWrite,
}

/// 单条能力声明：某后端支持的 `(kind, access)` 组合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VfsCapability {
    pub kind: VfsFsKind,
    pub access: VfsAccessMode,
}

impl VfsCapability {
    pub const fn new(kind: VfsFsKind, access: VfsAccessMode) -> Self {
        Self { kind, access }
    }
}
