//! 内核消息环存储 trait。
//! 本模块代码由AI完成

use crate::{AppendResult, KlogError, KlogRecordMeta};

/// 提交后的一条记录（正文切片指向环内存储）。
// 本结构代码由AI完成
pub struct KlogRecordView<'a> {
    /// 记录头。
    pub meta: KlogRecordMeta,
    /// 正文。
    pub text: &'a [u8],
}

/// 全局 klog 环契约（由 `klog-ringbuf` 实现）。
// 本结构代码由AI完成
pub trait KlogStore {
    /// 追加一条记录；`meta` 中 `seq` 由实现写入。
    fn append(&mut self, meta: &mut KlogRecordMeta, text: &[u8]) -> AppendResult;

    /// 统计快照。
    fn stats(&self) -> KlogStats;

    /// 用户态 READ 游标之后的未读正文字节总和（近似，用于 `SIZE_UNREAD`）。
    fn unread_bytes(&self) -> usize;

    /// `SIZE_BUFFER`：返回 text 环容量（Linux 语义近似为 log 缓冲区大小）。
    fn buffer_bytes(&self) -> usize;

    /// 取下一条未读记录（`seq == read_cursor` 的最小可用记录）。
    fn peek_next_unread(&self) -> Result<KlogRecordView<'_>, KlogError>;

    /// 推进读游标：`after_seq` 为刚消费完的序号；`clear_only` 为 true 时不要求曾 peek。
    fn advance_read_cursor(&mut self, after_seq: u64);

    /// 将读游标重置到当前最新之后（`CLEAR`：mark 全部已读，环保留）。
    fn clear_read_cursor(&mut self);
}

/// 环统计。
// 本结构代码由AI完成
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KlogStats {
    /// 成功提交条数。
    pub records_committed: u64,
    /// 因槽满覆盖丢弃的条数。
    pub records_dropped: u64,
    /// 当前最旧可见序号。
    pub oldest_seq: u64,
    /// 当前最新序号。
    pub newest_seq: u64,
    /// 下一条 READ 将返回的序号。
    pub read_cursor_seq: u64,
}
