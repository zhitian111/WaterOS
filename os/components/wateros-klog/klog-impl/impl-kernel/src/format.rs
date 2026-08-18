//! 不分配的日志正文与 traditional syslog 行格式化。

use core::fmt::{self, Write};

use api_v0::{AppendResult, KlogRecordMeta, LOG_KERN};

use crate::global::record;

/// `DATA:` 栈上格式化缓冲；正文超过容量时静默截断，避免日志路径分配或 panic。
struct KlogFmtBuffer {
    /// 预分配的栈缓冲，避免日志热路径依赖堆分配器或在 OOM 时递归记录错误。
    buf : [u8; Self::CAPACITY],
    /// 已写入的有效字节数，始终不超过 `buf.len()`。
    len : usize,
}

impl KlogFmtBuffer {
    /// 宏格式化正文的固定上限。
    const CAPACITY : usize = 512;

    /// 创建空缓冲；该操作可在早期启动阶段使用，不触及全局状态。
    const fn new() -> Self {
        Self { buf : [0; Self::CAPACITY],
               len : 0 }
    }

    /// 返回当前有效字节；格式化中途出错时仍可记录此前已经写入的前缀。
    fn as_bytes(&self) -> &[u8] { &self.buf[..self.len] }
}

impl Write for KlogFmtBuffer {
    fn write_str(&mut self, text : &str) -> fmt::Result {
        // `fmt::Write` 无法报告“被截断”。此处故意返回成功，让日志格式化不会因超长消息 panic。
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
    // 逐字节写入可同时处理任意二进制正文与很小的用户缓冲，无需 UTF-8 转换或临时分配。
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
