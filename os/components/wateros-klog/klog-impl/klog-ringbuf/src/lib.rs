#![no_std]
//! desc 槽 + 每槽变长正文（上限 `KLOG_MAX_RECORD_BYTES`）的环形 klog 实现。

use api_v0::{
    AppendResult, KlogError, KlogFlags, KlogRecordMeta, KlogRecordView, KlogStats, KlogStore,
};
use spin::Mutex;
use arch::interrupt::{
    disable_global_interrupt, read_global_interrupt_state, restore_global_interrupt_state,
    ArchInterruptState,
};
use wateros_base_config::klog::{
    KLOG_DESC_SLOTS, KLOG_MAX_RECORD_BYTES, KLOG_TEXT_RING_BYTES,
};

struct Slot {
    /// 槽是否持有有效记录（被覆盖后置 false）。
    valid: bool,
    meta: KlogRecordMeta,
    /// 单条正文上限 `KLOG_MAX_RECORD_BYTES`。
    bytes: [u8; KLOG_MAX_RECORD_BYTES],
}

/// 环内部状态（`KlogRingbuf::with` 闭包参数）。
pub struct KlogRingbufInner {
    slots: [Slot; KLOG_DESC_SLOTS],
    head: usize,
    count: usize,
    next_seq: u64,
    oldest_seq: u64,
    read_cursor_seq: u64,
    records_committed: u64,
    records_dropped: u64,
}

impl Default for KlogRingbufInner {
    fn default() -> Self {
        const EMPTY_SLOT: Slot = Slot {
            valid: false,
            meta: KlogRecordMeta {
                seq: 0,
                ts_nsec: 0,
                text_len: 0,
                facility: 0,
                flags: 0,
                level: 0,
                caller_id: 0,
            },
            bytes: [0; KLOG_MAX_RECORD_BYTES],
        };
        Self {
            slots: [EMPTY_SLOT; KLOG_DESC_SLOTS],
            head: 0,
            count: 0,
            next_seq: 1,
            oldest_seq: 0,
            read_cursor_seq: 1,
            records_committed: 0,
            records_dropped: 0,
        }
    }
}

impl KlogRingbufInner {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn slot_index_for_seq(&self, seq: u64) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        for i in 0..self.count {
            let idx = (self.head + KLOG_DESC_SLOTS - 1 - i) % KLOG_DESC_SLOTS;
            if self.slots[idx].valid && self.slots[idx].meta.seq == seq {
                return Some(idx);
            }
        }
        None
    }

    fn min_valid_seq(&self) -> Option<u64> {
        let mut min: Option<u64> = None;
        for i in 0..self.count {
            let idx = (self.head + KLOG_DESC_SLOTS - 1 - i) % KLOG_DESC_SLOTS;
            if self.slots[idx].valid {
                let s = self.slots[idx].meta.seq;
                min = Some(match min {
                    None => s,
                    Some(m) => m.min(s),
                });
            }
        }
        min
    }

    fn for_each_valid_seq<F>(&self, mut f: F)
    where
        F: FnMut(u64),
    {
        for i in 0..self.count {
            let idx = (self.head + KLOG_DESC_SLOTS - 1 - i) % KLOG_DESC_SLOTS;
            if self.slots[idx].valid {
                f(self.slots[idx].meta.seq);
            }
        }
    }

    fn refresh_oldest_seq(&mut self) {
        self.oldest_seq = self.min_valid_seq().unwrap_or(0);
    }

    /// 从 `start_seq` 起按序号升序访问仍存活的记录。
    pub fn iter_from<F>(&self, start_seq: u64, f: &mut F)
    where
        F: FnMut(KlogRecordView<'_>),
    {
        let mut buf = [0u64; KLOG_DESC_SLOTS];
        let mut n = 0usize;
        self.for_each_valid_seq(|seq| {
            if seq >= start_seq && n < KLOG_DESC_SLOTS {
                buf[n] = seq;
                n += 1;
            }
        });
        buf[..n].sort_unstable();
        for &seq in &buf[..n] {
            if let Some(idx) = self.slot_index_for_seq(seq) {
                let slot = &self.slots[idx];
                let len = slot.meta.text_len as usize;
                f(KlogRecordView {
                    meta: slot.meta,
                    text: &slot.bytes[..len],
                });
            }
        }
    }
}

impl KlogStore for KlogRingbufInner {
    #[inline]
    fn append(&mut self, meta: &mut KlogRecordMeta, text: &[u8]) -> AppendResult {
        let mut flags = KlogFlags(meta.flags);
        let copy_len = text.len().min(KLOG_MAX_RECORD_BYTES);
        let truncated = text.len() > copy_len;
        if truncated {
            flags = flags.with(KlogFlags::TRUNC);
        }

        let idx = self.head;
        if self.count == KLOG_DESC_SLOTS {
            if self.slots[idx].valid {
                self.records_dropped = self.records_dropped.saturating_add(1);
                let dropped_seq = self.slots[idx].meta.seq;
                if self.read_cursor_seq == dropped_seq {
                    self.read_cursor_seq = dropped_seq.saturating_add(1);
                }
            }
        } else {
            self.count += 1;
        }
        self.head = (self.head + 1) % KLOG_DESC_SLOTS;

        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        meta.seq = seq;
        meta.text_len = copy_len as u16;
        meta.flags = flags.0;

        let slot = &mut self.slots[idx];
        slot.valid = true;
        slot.meta = *meta;
        slot.bytes[..copy_len].copy_from_slice(&text[..copy_len]);

        self.refresh_oldest_seq();
        if self.oldest_seq != 0 && self.read_cursor_seq < self.oldest_seq {
            self.read_cursor_seq = self.oldest_seq;
        }

        self.records_committed = self.records_committed.saturating_add(1);
        AppendResult { seq, truncated }
    }

    #[inline]
    fn stats(&self) -> KlogStats {
        let newest = if self.records_committed == 0 {
            0
        } else {
            self.next_seq.saturating_sub(1)
        };
        KlogStats {
            records_committed: self.records_committed,
            records_dropped: self.records_dropped,
            oldest_seq: self.oldest_seq,
            newest_seq: newest,
            read_cursor_seq: self.read_cursor_seq,
        }
    }

    #[inline]
    fn unread_bytes(&self) -> usize {
        let mut sum = 0usize;
        self.for_each_valid_seq(|seq| {
            if seq >= self.read_cursor_seq {
                if let Some(idx) = self.slot_index_for_seq(seq) {
                    sum = sum.saturating_add(self.slots[idx].meta.text_len as usize);
                }
            }
        });
        sum
    }

    #[inline]
    fn buffer_bytes(&self) -> usize {
        KLOG_TEXT_RING_BYTES
    }

    fn peek_next_unread(&self) -> Result<KlogRecordView<'_>, KlogError> {
        let mut min_unread: Option<u64> = None;
        self.for_each_valid_seq(|seq| {
            if seq >= self.read_cursor_seq {
                min_unread = Some(match min_unread {
                    None => seq,
                    Some(m) => m.min(seq),
                });
            }
        });
        if let Some(seq) = min_unread {
            if let Some(idx) = self.slot_index_for_seq(seq) {
                let slot = &self.slots[idx];
                let len = slot.meta.text_len as usize;
                return Ok(KlogRecordView {
                    meta: slot.meta,
                    text: &slot.bytes[..len],
                });
            }
        }
        Err(KlogError::NoUnread)
    }

    fn advance_read_cursor(&mut self, after_seq: u64) {
        self.read_cursor_seq = after_seq.saturating_add(1);
        if self.oldest_seq != 0 && self.read_cursor_seq < self.oldest_seq {
            self.read_cursor_seq = self.oldest_seq;
        }
    }

    fn clear_read_cursor(&mut self) {
        let newest = self.next_seq.saturating_sub(1);
        self.read_cursor_seq = newest.saturating_add(1);
    }
}

/// 全局内核消息环。
pub struct KlogRingbuf;

static KLOG: Mutex<Option<KlogRingbufInner>> = Mutex::new(None);

struct KlogInterruptGuard {
    state : ArchInterruptState,
}

impl KlogInterruptGuard {
    fn new() -> Self {
        let state = read_global_interrupt_state()
            .expect("read global interrupt state for klog guard");
        disable_global_interrupt().expect("disable global interrupt for klog guard");
        Self { state }
    }
}

impl Drop for KlogInterruptGuard {
    fn drop(&mut self) {
        restore_global_interrupt_state(self.state)
            .expect("restore global interrupt state for klog guard");
    }
}

fn ensure_inner(guard: &mut Option<KlogRingbufInner>) -> &mut KlogRingbufInner {
    if guard.is_none() {
        *guard = Some(KlogRingbufInner::default());
    }
    guard.as_mut().unwrap()
}

impl KlogRingbuf {
    /// 初始化全局环（可重复调用，会清空内容）。
    #[inline]
    pub fn init() {
        let mut guard = KLOG.lock();
        let inner = ensure_inner(&mut guard);
        inner.reset();
    }

    /// 在已持有锁的闭包内访问环。
    #[inline]
    pub fn with<R>(f: impl FnOnce(&mut KlogRingbufInner) -> R) -> R {
        let _irq = KlogInterruptGuard::new();
        let mut guard = KLOG.lock();
        f(ensure_inner(&mut guard))
    }

    /// 从 `start_seq` 起迭代记录。
    pub fn iter_from<F>(start_seq: u64, mut f: F)
    where
        F: FnMut(KlogRecordView<'_>),
    {
        let _irq = KlogInterruptGuard::new();
        let mut guard = KLOG.lock();
        ensure_inner(&mut guard).iter_from(start_seq, &mut f);
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use api_v0::{KlogRecordMeta, LOG_INFO, LOG_KERN};

    #[test]
    fn append_and_unread() {
        KlogRingbuf::init();
        KlogRingbuf::with(|ring| {
            let mut meta = KlogRecordMeta::new(0, 0, LOG_KERN, LOG_INFO, KlogFlags::empty(), 0);
            ring.append(&mut meta, b"hello");
            assert_eq!(ring.unread_bytes(), 5);
            let view = ring.peek_next_unread().unwrap();
            assert_eq!(view.text, b"hello");
            ring.advance_read_cursor(view.meta.seq);
            assert_eq!(ring.unread_bytes(), 0);
        });
    }
}
