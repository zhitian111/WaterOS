use alloc::string::String;

/// 文件系统操作错误；实现方将底层 I/O 与格式错误映射到此枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    /// 根卷未挂载或句柄未就绪。
    NotMounted,
    /// 路径不存在。
    NotFound,
    /// 期望文件但目标为目录或特殊节点等。
    NotAFile,
    /// 路径非法、过长或不符合实现约束。
    InvalidPath,
    /// 目标路径已存在（如 `mkdir` 时目录项冲突）。
    Exists,
    /// 目录非空，无法删除。
    NotEmpty,
    /// 内容非合法 UTF-8（如 `read_to_string`）。
    NotUtf8,
    /// 操作或组合不被当前实现支持。
    Unsupported,
    /// 块设备驱动返回错误。
    Driver,
    /// 卷元数据或结构损坏。
    Corrupt,
    /// 通用 I/O 失败（非驱动分类错误）。
    Io,
    /// 无剩余空间；cgroup cpuset 无可用 cpus/mems 时拒绝 attach 也用此语义。
    NoSpace,
}

/// [`FsError`] 上的结果别名。
pub type FsResult<T> = core::result::Result<T, FsError>;

/// 文件系统类型标识。`Other` 用于子系统级别的虚拟 FS（如 devfs），便于注册表统一登记。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FsKind {
    /// ext2 族（探测或能力声明用）。
    Ext2,
    /// ext3 族。
    Ext3,
    /// ext4（当前 RO/RW 实现主要对应此 kind）。
    Ext4,
    /// 内核设备文件树（非块卷 FS）。
    DevFs,
    /// 物理页 payload 后端 ramfs；tmpfs 由 VFS 策略层基于它创建挂载实例。
    RamFs,
    /// 其他具名子系统；字符串为稳定展示名。
    Other(&'static str),
}

/// 文件系统挂载访问模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FsAccessMode {
    /// 只读挂载。
    ReadOnly,
    /// 读写挂载。
    ReadWrite,
}

/// 单条能力声明：某 impl 支持的 (FsKind, FsAccessMode) 组合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FsCapability {
    /// 文件系统种类。
    pub kind: FsKind,
    /// 访问模式。
    pub access: FsAccessMode,
}

impl FsCapability {
    /// 构造一条 `(kind, access)` 能力声明，供 impl 的 `supported()` 静态表使用。
    pub const fn new(kind: FsKind, access: FsAccessMode) -> Self { Self { kind, access } }
}

/// VFS/调试用的节点类型分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsNodeType {
    /// 普通文件。
    File,
    /// 目录。
    Directory,
    /// 符号链接。
    Symlink,
    /// 其他特殊 inode（设备节点等），具体语义依赖实现。
    Special,
}

/// 路径对应的元数据快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsMetadata {
    /// 节点类型。
    pub node_type: FsNodeType,
    /// 以字节为单位的大小（目录实现可能为 0 或近似值）。
    pub size: u64,
    /// Unix 风格 mode 位（实现相关）。
    pub mode: u16,
    /// 文件系统内部 inode 编号；同一文件的硬链接必须返回相同编号。
    pub inode: u64,
    /// 指向该 inode 的硬链接数量。
    pub nlink: u32,
    /// 属主 uid（Linux `st_uid`）。
    pub uid: u32,
    /// 属组 gid（Linux `st_gid`）。
    pub gid: u32,
}

/// 文件系统实例内稳定的节点身份。
///
/// 该值只在创建它的挂载实例及对应 open/close 生命周期内有效。调用方必须同时保存
/// mount generation，不得把它跨卸载复用。具体数值由实现解释，不泄漏后端 inode 类型。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FsNodeId(u64);

impl FsNodeId {
    /// 由文件系统实现构造实例内节点身份。
    pub const fn new(raw: u64) -> Self { Self(raw) }

    /// 返回用于缓存键和诊断的实例内数值；不能据此绕过文件系统方法访问节点。
    pub const fn raw(self) -> u64 { self.0 }
}

/// 根卷文件 I/O 模式（与 `wateros-base-config::fs::FileIoMode` 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsIoMode {
    /// 同步区间读写。
    Direct,
    /// 异步区间读写（v1 未实现）。
    Async,
}

/// 目录枚举单条结果：仅含名字与类型（不含完整路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsDirEntry {
    /// 目录项名字（非路径）。
    pub name: String,
    /// 节点类型。
    pub node_type: FsNodeType,
}
