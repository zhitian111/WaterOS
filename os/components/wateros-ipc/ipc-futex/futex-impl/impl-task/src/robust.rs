//! 内核侧 per-task robust 状态（不含用户链表内容）。

/// 已校验的 robust 链表头用户地址与长度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RobustState {
    /// 用户态 `struct robust_list_head` 地址。
    pub head: usize,
    /// 头结构长度（应为 24）。
    pub len: usize,
}
