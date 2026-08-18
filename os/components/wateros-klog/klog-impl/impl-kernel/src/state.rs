//! 固定槽环的状态、覆盖策略和全局 read cursor 语义。

use api_v0::{
    AppendResult, KlogError, KlogFlags, KlogRecordMeta, KlogRecordView, KlogStats, KlogStore,
};
use wateros_base_config::klog::{KLOG_DESC_SLOTS, KLOG_MAX_RECORD_BYTES, KLOG_TEXT_RING_BYTES};

/// `DATA:` 一个 descriptor 槽及其固定上限正文存储。
#[derive(Clone, Copy)]
struct Slot {
    /// 此槽是否含有已提交记录；复位后的槽没有可读序号。
    valid : bool,
    /// 正文的固定元数据副本，只有 `valid` 时才有意义。
    meta : KlogRecordMeta,
    /// 固定容量正文存储；有效前缀长度由 `meta.text_len` 指定，其余字节无需清零语义。
    bytes : [u8; KLOG_MAX_RECORD_BYTES],
}

/// `DATA:` 环的全部可变状态；只能通过 `KlogRingbuf` 在全局锁内访问。
///
/// `INVARIANT:` 有效记录按 sequence 单调递增；`head` 指向下一次写入位置。满环写入会覆盖
/// 最旧槽并推进读游标，保证 `read_cursor_seq` 不会永久指向已丢失记录。
pub(crate) struct KlogRingbufInner {
    /// 描述符与正文槽，按 `head` 环绕复用；容量为零是配置错误，不是空环的表示。
    slots : [Slot; KLOG_DESC_SLOTS],
    /// 下一次覆盖写入的槽下标；写入后立即环绕，始终小于槽容量。
    head : usize,
    /// 当前有效记录数，范围为 `0..=KLOG_DESC_SLOTS`。
    count : usize,
    /// 下一条提交记录要使用的序号；饱和后不再递增，极端寿命下序号不再严格唯一。
    next_seq : u64,
    /// 当前仍保留的最小序号；空环为 0，不能把 0 当作可读记录。
    oldest_seq : u64,
    /// 下一条逻辑未读记录的最小序号；覆盖旧记录时会前移，避免游标永久悬空。
    read_cursor_seq : u64,
    /// 自本次初始化以来成功提交的累计条数，饱和而非回绕。
    records_committed : u64,
    /// 因满环覆盖而不可再读的记录条数；单条正文截断不计入此数。
    records_dropped : u64,
}

impl Default for KlogRingbufInner {
    fn default() -> Self {
        const EMPTY_SLOT : Slot = Slot { valid : false,
                                         meta : KlogRecordMeta { seq : 0,
                                                                 ts_nsec : 0,
                                                                 text_len : 0,
                                                                 facility : 0,
                                                                 flags : 0,
                                                                 level : 0,
                                                                 caller_id : 0 },
                                         bytes : [0; KLOG_MAX_RECORD_BYTES] };
        Self { slots : [EMPTY_SLOT; KLOG_DESC_SLOTS],
               head : 0,
               count : 0,
               next_seq : 1,
               oldest_seq : 0,
               read_cursor_seq : 1,
               records_committed : 0,
               records_dropped : 0 }
    }
}

impl KlogRingbufInner {
    /// 清空记录与读游标；仅启动或显式重新初始化时使用。
    pub(crate) fn reset(&mut self) { *self = Self::default(); }

    /// 反向遍历有效槽定位指定序号；容量很小，选择线性扫描以避免额外索引在覆盖时失配。
    fn slot_index_for_seq(&self, seq : u64) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        for offset in 0..self.count {
            let index = (self.head + KLOG_DESC_SLOTS - 1 - offset) % KLOG_DESC_SLOTS;
            if self.slots[index].valid && self.slots[index].meta.seq == seq {
                return Some(index);
            }
        }
        None
    }

    /// 对每个有效序号调用闭包。顺序是从最新到最旧，调用方不得假定为读取顺序。
    fn for_each_valid_seq(&self, mut f : impl FnMut(u64)) {
        for offset in 0..self.count {
            let index = (self.head + KLOG_DESC_SLOTS - 1 - offset) % KLOG_DESC_SLOTS;
            if self.slots[index].valid {
                f(self.slots[index].meta.seq);
            }
        }
    }

    /// 覆盖写后重新计算最旧序号，防止依赖槽下标推导时在环绕处产生错误。
    fn refresh_oldest_seq(&mut self) {
        self.oldest_seq = (0..self.count)
            .filter_map(|offset| {
                let index = (self.head + KLOG_DESC_SLOTS - 1 - offset) % KLOG_DESC_SLOTS;
                self.slots[index].valid.then_some(self.slots[index].meta.seq)
            })
            .min()
            .unwrap_or(0);
    }

}

impl KlogStore for KlogRingbufInner {
    /// `FLOW:` 覆盖写 head 槽；满环时计 dropped，必要时推进读游标，再发布新 sequence。
    fn append(&mut self, meta : &mut KlogRecordMeta, text : &[u8]) -> AppendResult {
        // 先确定可保存正文长度，再更新调用者可见的元数据，确保返回后长度与标志彼此一致。
        let mut flags = KlogFlags(meta.flags);
        let copy_len = text.len().min(KLOG_MAX_RECORD_BYTES);
        let truncated = text.len() > copy_len;
        if truncated {
            flags = flags.with(KlogFlags::TRUNC);
        }

        let index = self.head;
        // `head` 在满环时恰好指向最旧槽。覆盖它之前推进正在指向它的读游标，避免返回已覆盖正文。
        if self.count == KLOG_DESC_SLOTS {
            if self.slots[index].valid {
                self.records_dropped = self.records_dropped.saturating_add(1);
                let dropped_seq = self.slots[index].meta.seq;
                if self.read_cursor_seq == dropped_seq {
                    self.read_cursor_seq = dropped_seq.saturating_add(1);
                }
            }
        } else {
            self.count += 1;
        }
        self.head = (self.head + 1) % KLOG_DESC_SLOTS;

        meta.seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        meta.text_len = copy_len as u16;
        meta.flags = flags.0;
        self.slots[index] = Slot { valid : true,
                                   meta : *meta,
                                   bytes : [0; KLOG_MAX_RECORD_BYTES] };
        self.slots[index].bytes[..copy_len].copy_from_slice(&text[..copy_len]);

        // 写入完成后才公开新槽的最旧边界；所有这些修改均受全局 klog 锁保护。
        self.refresh_oldest_seq();
        if self.oldest_seq != 0 && self.read_cursor_seq < self.oldest_seq {
            self.read_cursor_seq = self.oldest_seq;
        }
        self.records_committed = self.records_committed.saturating_add(1);
        AppendResult { seq : meta.seq,
                       truncated }
    }

    /// 在锁内复制统计字段；不借出任何环内存。
    fn stats(&self) -> KlogStats {
        KlogStats { records_committed : self.records_committed,
                    records_dropped : self.records_dropped,
                    oldest_seq : self.oldest_seq,
                    newest_seq : if self.records_committed == 0 {
                        0
                    } else {
                        self.next_seq.saturating_sub(1)
                    },
                    read_cursor_seq : self.read_cursor_seq }
    }

    /// 汇总未读正文而不包含 traditional 前缀或换行，和 `SIZE_UNREAD` 的近似 ABI 语义一致。
    fn unread_bytes(&self) -> usize {
        let mut total = 0usize;
        self.for_each_valid_seq(|seq| {
            if seq >= self.read_cursor_seq {
                if let Some(index) = self.slot_index_for_seq(seq) {
                    total = total.saturating_add(self.slots[index].meta.text_len as usize);
                }
            }
        });
        total
    }

    /// 返回配置声明的正文环容量；当前固定槽实现可能因每槽预留空间而不能精确反映实际占用。
    fn buffer_bytes(&self) -> usize { KLOG_TEXT_RING_BYTES }

    /// 借出最旧未读槽。返回的视图只能在调用方仍持有 klog 锁时使用。
    fn peek_next_unread(&self) -> Result<KlogRecordView<'_>, KlogError> {
        let next = (0..self.count)
            .filter_map(|offset| {
                let index = (self.head + KLOG_DESC_SLOTS - 1 - offset) % KLOG_DESC_SLOTS;
                let slot = &self.slots[index];
                (slot.valid && slot.meta.seq >= self.read_cursor_seq).then_some(slot.meta.seq)
            })
            .min();
        let Some(sequence) = next else {
            return Err(KlogError::NoUnread);
        };
        let index = self.slot_index_for_seq(sequence).ok_or(KlogError::NoUnread)?;
        let slot = &self.slots[index];
        Ok(KlogRecordView { meta : slot.meta,
                            text : &slot.bytes[..slot.meta.text_len as usize] })
    }

    /// 消费指定序号并推进游标；若覆盖已推进最旧边界，则再次钳制避免倒退。
    fn advance_read_cursor(&mut self, after_seq : u64) {
        self.read_cursor_seq = after_seq.saturating_add(1);
        if self.oldest_seq != 0 && self.read_cursor_seq < self.oldest_seq {
            self.read_cursor_seq = self.oldest_seq;
        }
    }

    /// 标记当前全部记录已读；不擦除槽，故统计和内核诊断仍可观察历史记录。
    fn clear_read_cursor(&mut self) {
        self.read_cursor_seq = self.next_seq;
    }
}
