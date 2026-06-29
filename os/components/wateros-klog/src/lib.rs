#![no_std]
//! 内核消息环聚合层：全局环、`klog_*!` 宏与 `sys_syslog` 内核语义。

pub use api_v0 as api;
pub use impl_ringbuf::KlogRingbuf;

pub mod export;
pub mod syscall;

use api_v0::{
    AppendResult, KlogFlags, KlogRecordMeta, KlogStats, KlogStore, LOG_INFO, LOG_KERN,
};
use impl_ringbuf::KlogRingbuf as Ring;

/// 初始化全局 klog 环（清空内容；不写入消息）。
#[inline]
pub fn init() {
    Ring::init();
}

/// 内核主线初始化完成后调用，写入固定问候语供 `syslog(2)` / `dmesg` 读取。
#[inline]
pub fn post_init_hello() {
    let _ = record(LOG_INFO, LOG_KERN, b"hello wateros\n");
}

/// 追加一条记录（自动填时间戳与 `caller_id`）。
#[inline]
pub fn record(level: u8, facility: u8, text: &[u8]) -> AppendResult {
    let mut meta = KlogRecordMeta::new(
        ts_nsec_now(),
        0,
        facility,
        level,
        KlogFlags::empty(),
        caller_id_now(),
    );
    record_with_meta(&mut meta, text)
}

/// 使用已填字段的 meta 追加（`text_len` 由环覆盖）。
#[inline]
pub fn record_with_meta(meta: &mut KlogRecordMeta, text: &[u8]) -> AppendResult {
    Ring::with(|ring| ring.append(meta, text))
}

/// 统计快照。
#[inline]
pub fn stats() -> KlogStats {
    Ring::with(|ring| ring.stats())
}

/// 从 `start_seq` 起迭代记录。
#[inline]
pub fn iter_from<F>(start_seq: u64, f: F)
where
    F: FnMut(api_v0::KlogRecordView<'_>),
{
    Ring::iter_from(start_seq, f);
}

/// 单调时钟纳秒；失败时为 0。
#[inline]
pub fn ts_nsec_now() -> u64 {
    platform::timer::now_duration()
        .map(|d| {
            d.as_secs()
                .saturating_mul(1_000_000_000)
                .saturating_add(d.subsec_nanos() as u64)
        })
        .unwrap_or(0)
}

/// 当前任务 ID；无调度上下文时为 0。
#[inline]
pub fn caller_id_now() -> u32 {
    task::current_task_id().map(|id| id as u32).unwrap_or(0)
}

#[macro_export]
macro_rules! klog_trace {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let mut s = $crate::KlogFmtBuffer::new();
        let _ = write!(s, $($arg)*);
        let _ = $crate::record($crate::api::LOG_DEBUG, $crate::api::LOG_KERN, s.as_bytes());
    }};
}

#[macro_export]
macro_rules! klog_debug {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let mut s = $crate::KlogFmtBuffer::new();
        let _ = write!(s, $($arg)*);
        let _ = $crate::record($crate::api::LOG_DEBUG, $crate::api::LOG_KERN, s.as_bytes());
    }};
}

#[macro_export]
macro_rules! klog_info {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let mut s = $crate::KlogFmtBuffer::new();
        let _ = write!(s, $($arg)*);
        let _ = $crate::record($crate::api::LOG_INFO, $crate::api::LOG_KERN, s.as_bytes());
    }};
}

#[macro_export]
macro_rules! klog_warn {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let mut s = $crate::KlogFmtBuffer::new();
        let _ = write!(s, $($arg)*);
        let _ = $crate::record($crate::api::LOG_WARNING, $crate::api::LOG_KERN, s.as_bytes());
    }};
}

#[macro_export]
macro_rules! klog_error {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let mut s = $crate::KlogFmtBuffer::new();
        let _ = write!(s, $($arg)*);
        let _ = $crate::record($crate::api::LOG_ERR, $crate::api::LOG_KERN, s.as_bytes());
    }};
}

/// 栈上格式化缓冲（供宏使用，上限 512 字节）。
pub struct KlogFmtBuffer {
    buf: [u8; 512],
    len: usize,
}

impl KlogFmtBuffer {
    /// 空缓冲。
    #[inline]
    pub const fn new() -> Self {
        Self { buf: [0; 512], len: 0 }
    }

    /// 已写入切片。
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl core::fmt::Write for KlogFmtBuffer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let room = self.buf.len().saturating_sub(self.len);
        let n = bytes.len().min(room);
        self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
        Ok(())
    }
}
