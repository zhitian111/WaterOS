//! 环操作错误（内核侧；syscall 层映射为 errno 或 panic）。

/// `KlogStore` 操作结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KlogError {
    /// 无未读记录（READ 路径返回 0，非错误）。
    NoUnread,
    /// 用户/调用方缓冲过小。
    BufferTooSmall,
}

/// 追加结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppendResult {
    /// 分配到的序号。
    pub seq: u64,
    /// 是否发生截断。
    pub truncated: bool,
}
