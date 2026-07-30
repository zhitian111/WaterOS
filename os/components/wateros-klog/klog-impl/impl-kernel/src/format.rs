//! 不分配的日志正文与 traditional syslog 行格式化。

use core::fmt::{self, Write};

use api_v0::{AppendResult, KlogRecordMeta, LOG_KERN};

use crate::global::record;

/// `DATA:` 栈上格式化缓冲；正文超过容量时静默截断，避免日志路径分配或 panic。
struct KlogFmtBuffer {
    buf : [u8; Self::CAPACITY],
    len : usize,
}

impl KlogFmtBuffer {
    /// 宏格式化正文的固定上限。
    const CAPACITY : usize = 512;

    const fn new() -> Self {
        Self { buf : [0; Self::CAPACITY],
               len : 0 }
    }

    fn as_bytes(&self) -> &[u8] { &self.buf[..self.len] }
}

impl Write for KlogFmtBuffer {
    fn write_str(&mut self, text : &str) -> fmt::Result {
        let room = self.buf.len().saturating_sub(self.len);
        let copy_len = text.len().min(room);
        self.buf[self.len..self.len + copy_len].copy_from_slice(&text.as_bytes()[..copy_len]);
        self.len += copy_len;
        Ok(())
    }
}

/// `FLOW:` 将 `format_args!` 结果写入固定栈缓冲后追加内核记录；不分配、不会调用 console。
pub fn record_fmt(level : u8, arguments : fmt::Arguments<'_>) -> AppendResult {
    let mut buffer = KlogFmtBuffer::new();
    let _ = buffer.write_fmt(arguments);
    record(level, LOG_KERN, buffer.as_bytes())
}

/// `ABI:` traditional syslog 行格式：`<level>text\n`。
///
/// 返回写入 `out` 的字节数；缓冲不足时截断并仍尽量以 `\n` 结尾。
#[must_use]
pub(crate) fn format_traditional(meta : &KlogRecordMeta, text : &[u8], out : &mut [u8]) -> usize {
    let prefix = [b'<', meta.traditional_level_char(), b'>'];
    let mut written = 0usize;
    for byte in prefix.iter().chain(text.iter()).chain(core::iter::once(&b'\n')) {
        if written == out.len() {
            break;
        }
        out[written] = *byte;
        written += 1;
    }
    if written == out.len() && written > 0 {
        out[written - 1] = b'\n';
    }
    written
}
