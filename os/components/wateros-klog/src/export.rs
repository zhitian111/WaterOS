//! 用户态可见的 syslog 线格式（traditional）。

use api_v0::KlogRecordMeta;

/// 将一条记录格式化为 `"<N>..."` 传统 syslog 行（含末尾 `\n`）。
///
/// 返回写入 `out` 的字节数；缓冲不足时截断并仍尽量以 `\n` 结尾。
#[must_use]
pub fn format_traditional(meta: &KlogRecordMeta, text: &[u8], out: &mut [u8]) -> usize {
    let level_ch = [meta.traditional_level_char()];
    let prefix = [b'<', level_ch[0], b'>'];
    let mut written = 0usize;
    for &b in &prefix {
        if written >= out.len() {
            break;
        }
        out[written] = b;
        written += 1;
    }
    let text_room = out.len().saturating_sub(written).saturating_sub(1);
    let text_len = text.len().min(text_room);
    out[written..written + text_len].copy_from_slice(&text[..text_len]);
    written += text_len;
    if written < out.len() {
        out[written] = b'\n';
        written += 1;
    } else if !out.is_empty() {
        out[out.len() - 1] = b'\n';
        written = out.len();
    }
    written
}
