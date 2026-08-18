//! 环操作错误（内核侧；syscall 层映射为 errno 或 panic）。

/// `KlogStore` 操作结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KlogError {
    /// 无未读记录（READ 路径返回 0，非错误）。
    NoUnread,
    /// 调用方缓冲过小，无法按接口要求写入完整结果。
    ///
    /// 当前内核实现会截断传统 syslog 行而不是产生此错误；保留该变体是为了让其他
    /// `KlogStore` 实现能够选择“必须完整记录”的契约。
    BufferTooSmall,
}

/// 追加结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppendResult {
    /// 分配到的单调递增序号；重置日志服务后序号会从初始值重新开始，不能跨重置比较。
    pub seq: u64,
    /// 是否因单条记录固定上限而丢弃了正文尾部；调用者仍可将本次追加视为成功。
    pub truncated: bool,
}
