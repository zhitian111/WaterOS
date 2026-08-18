//! 内核消息环存储 trait。

use crate::{AppendResult, KlogError, KlogRecordMeta};

/// `DATA:` 提交后的一条记录视图（正文切片借用环内存储）。
///
/// 视图只能在产生它的 store 锁闭包内使用；不得跨越下一次 append、解锁或调度保存它。
pub struct KlogRecordView<'a> {
    /// 记录头的锁内副本；`seq`、长度和截断标志反映实际提交结果。
    pub meta: KlogRecordMeta,
    /// 借用环槽的正文，长度等于 `meta.text_len`；它不是以 NUL 结尾的字符串。
    pub text: &'a [u8],
}

/// 全局 klog 环契约（由 `klog-impl/impl-kernel` 实现）。
///
/// `LOCK:` 本 trait 的引用视图方法要求调用方维持实现的互斥保护；实现可用全局锁，但不应把
/// 锁策略暴露给 API 消费者。
pub trait KlogStore {
    /// 追加一条记录；`meta` 中 `seq`、`text_len` 与截断标志由实现写入。
    ///
    /// `text` 可以是任意字节而非 UTF-8。记录过长时实现可截断但必须在返回值和元数据中如实报告。
    fn append(&mut self, meta: &mut KlogRecordMeta, text: &[u8]) -> AppendResult;

    /// 取得统计快照；返回后允许并发追加，因此它只描述某一瞬间，不能作为后续读取的承诺。
    fn stats(&self) -> KlogStats;

    /// 用户态 READ 游标之后的未读正文字节总和（近似，用于 `SIZE_UNREAD`）。
    fn unread_bytes(&self) -> usize;

    /// `SIZE_BUFFER`：返回 text 环容量（Linux 语义近似为 log 缓冲区大小）。
    fn buffer_bytes(&self) -> usize;

    /// 取下一条未读记录（sequence 不小于 read cursor 的最小可用记录）。
    fn peek_next_unread(&self) -> Result<KlogRecordView<'_>, KlogError>;

    /// 推进读游标：`after_seq` 为刚消费完的序号。
    ///
    /// 传入已被覆盖或未来的序号由实现钳制到仍有效的区间；调用者通常只能传入刚由
    /// [`Self::peek_next_unread`] 返回的 `meta.seq`。
    fn advance_read_cursor(&mut self, after_seq: u64);

    /// 将读游标重置到当前最新之后（`CLEAR`：mark 全部已读，环保留）。
    fn clear_read_cursor(&mut self);
}

/// `DATA:` 环统计快照；值在复制时一致，但不会阻止后续并发追加或覆盖。
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
