//! VFS 统一错误面。

/// VFS 路径与 I/O 操作的统一错误；与具体 FS 后端映射由 `vfs-impl-*` 保证语义对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    /// 根卷或所需设备未就绪、未挂载。
    NotMounted,
    /// 规范化路径下无对应节点。
    NotFound,
    /// 操作要求普通文件，当前节点类型不符。
    NotAFile,
    /// 路径非法（如非绝对、空段约定违反、根文件名含 `/` 等）。
    InvalidPath,
    /// 目标路径已存在。
    Exists,
    /// 按 UTF-8 解释文件内容失败。
    NotUtf8,
    /// 当前构建或后端不支持该操作。
    Unsupported,
    /// 块设备或驱动层错误。
    Driver,
    /// 元数据或结构与后端约定不一致。
    Corrupt,
    /// 读写字节与预期不一致等泛 I/O 语义失败。
    Io,
    /// 文件描述符无效或已关闭。
    BadFd,
    /// 目标 fd 正被另一路径用于阻塞 I/O，暂不可变更（如 close/dup）。
    Busy,
    /// 非阻塞 I/O 暂不可用（如 pipe 读/写）。
    WouldBlock,
    /// 阻塞操作被信号中断。
    Interrupted,
    /// 管道对端已关闭。
    BrokenPipe,
    /// 无当前任务上下文（如 `ESRCH` 语义）。
    NoTask,
    /// 进程 `RLIMIT_NOFILE` 已达上限。
    TooManyOpenFiles,
    /// 目标位于只读挂载卷上，拒绝写或创建。
    ReadOnlyFs,
    /// 设备无剩余空间；cgroup cpuset 无可用 cpus/mems 时拒绝 attach 也用此语义。
    NoSpace,
}

/// [`VfsError`] 上的 [`core::result::Result`] 别名。
pub type VfsResult<T> = core::result::Result<T, VfsError>;
