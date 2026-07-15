//! 后端文件系统种类与能力声明（VFS 自有类型，不依赖 `wateros-fs`）。

/// 文件系统类型标识（VFS 命名空间）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VfsFsKind {
    /// ext2 根卷或辅助卷。
    Ext2,
    /// ext3。
    Ext3,
    /// ext4（当前根卷默认）。
    Ext4,
    /// heap-backed ramfs；tmpfs 策略层可基于它创建挂载。
    RamFs,
    /// 其他具名子系统。
    Other(&'static str),
}

/// 挂载访问模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VfsAccessMode {
    /// 只读挂载。
    ReadOnly,
    /// 读写挂载。
    ReadWrite,
}

/// 单条能力声明：某后端支持的 `(kind, access)` 组合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VfsCapability {
    /// 文件系统种类。
    pub kind: VfsFsKind,
    /// 访问模式。
    pub access: VfsAccessMode,
}

impl VfsCapability {
    /// 构造一条 `(kind, access)` 能力声明。
    pub const fn new(kind: VfsFsKind, access: VfsAccessMode) -> Self {
        Self { kind, access }
    }
}
