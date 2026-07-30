//! 固定槽环的状态、覆盖策略和全局 read cursor 语义。

use api_v0::{
    AppendResult, KlogError, KlogFlags, KlogRecordMeta, KlogRecordView, KlogStats, KlogStore,
};
use wateros_base_config::klog::{KLOG_DESC_SLOTS, KLOG_MAX_RECORD_BYTES, KLOG_TEXT_RING_BYTES};

/// `DATA:` 一个 descriptor 槽及其固定上限正文存储。
#[derive(Clone, Copy)]
struct Slot {
    valid : bool,
    meta : KlogRecordMeta,
    bytes : [u8; KLOG_MAX_RECORD_BYTES],
}

/// `DATA:` 环的全部可变状态；只能通过 `KlogRingbuf` 在全局锁内访问。
///
/// `INVARIANT:` 有效记录按 sequence 单调递增；`head` 指向下一次写入位置。满环写入会覆盖
/// 最旧槽并推进读游标，保证 `read_cursor_seq` 不会永久指向已丢失记录。
pub(crate) struct KlogRingbufInner {
    slots : [Slot; KLOG_DESC_SLOTS],
    head : usize,
    count : usize,
    next_seq : u64,
    oldest_seq : u64,
    read_cursor_seq : u64,
    records_committed : u64,
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

    fn for_each_valid_seq(&self, mut f : impl FnMut(u64)) {
        for offset in 0..self.count {
            let index = (self.head + KLOG_DESC_SLOTS - 1 - offset) % KLOG_DESC_SLOTS;
            if self.slots[index].valid {
                f(self.slots[index].meta.seq);
            }
        }
    }

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
        let mut flags = KlogFlags(meta.flags);
        let copy_len = text.len().min(KLOG_MAX_RECORD_BYTES);
        let truncated = text.len() > copy_len;
        if truncated {
            flags = flags.with(KlogFlags::TRUNC);
        }

        let index = self.head;
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

        self.refresh_oldest_seq();
        if self.oldest_seq != 0 && self.read_cursor_seq < self.oldest_seq {
            self.read_cursor_seq = self.oldest_seq;
        }
        self.records_committed = self.records_committed.saturating_add(1);
        AppendResult { seq : meta.seq,
                       truncated }
    }

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

    fn buffer_bytes(&self) -> usize { KLOG_TEXT_RING_BYTES }

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

    fn advance_read_cursor(&mut self, after_seq : u64) {
        self.read_cursor_seq = after_seq.saturating_add(1);
        if self.oldest_seq != 0 && self.read_cursor_seq < self.oldest_seq {
            self.read_cursor_seq = self.oldest_seq;
        }
    }

    fn clear_read_cursor(&mut self) {
        self.read_cursor_seq = self.next_seq;
    }
}
