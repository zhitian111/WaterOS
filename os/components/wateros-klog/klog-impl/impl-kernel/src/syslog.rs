//! `sys_syslog` 的内核缓冲语义；用户内存拷贝由 syscall 层完成。

use api_v0::{
    is_write_priority, KlogError, KlogFlags, KlogRecordMeta, KlogStore, SYSLOG_ACTION_CLEAR,
    SYSLOG_ACTION_CLOSE, SYSLOG_ACTION_CONSOLE_LEVEL, SYSLOG_ACTION_CONSOLE_OFF,
    SYSLOG_ACTION_CONSOLE_ON, SYSLOG_ACTION_OPEN, SYSLOG_ACTION_READ, SYSLOG_ACTION_READ_ALL,
    SYSLOG_ACTION_READ_CLEAR, SYSLOG_ACTION_SIZE_BUFFER, SYSLOG_ACTION_SIZE_UNREAD,
};
use crate::format::format_traditional;
use crate::global::{caller_id_now, record_with_meta, ts_nsec_now, KlogRingbuf};

/// 单次传统 syslog 格式化的内核栈缓冲上限；它包含 `<level>` 前缀和结尾换行。
const KERNEL_LINE_MAX : usize = 2048;

/// `FLOW:` 处理 `sys_syslog` action；`kernel_buf` 是 syscall 层提供的内核缓冲。
///
/// `LOCK:` 单条记录的读取、格式化与 cursor 推进在同一个 ring 锁闭包内完成，避免读取到已被
/// 覆盖的 text。调用本函数前不得持有会被日志路径重入的锁。
pub(crate) fn dispatch_kernel(action : i32, kernel_buf : &mut [u8], kernel_len : usize) -> isize {
    if is_write_priority(action) {
        return write_priority(action, kernel_buf, kernel_len);
    }
    match action {
        // 尚未实现每文件描述符会话状态，OPEN/CLOSE 仅作为成功的 ABI 兼容操作。
        SYSLOG_ACTION_CLOSE | SYSLOG_ACTION_OPEN => 0,
        SYSLOG_ACTION_SIZE_UNREAD => KlogRingbuf::with(|ring| ring.unread_bytes() as isize),
        SYSLOG_ACTION_SIZE_BUFFER => KlogRingbuf::with(|ring| ring.buffer_bytes() as isize),
        SYSLOG_ACTION_CLEAR => {
            KlogRingbuf::with(|ring| ring.clear_read_cursor());
            0
        }
        // console 输出由 runtime 管理，不能让未实现的 syslog 控制操作改变全局日志策略。
        SYSLOG_ACTION_CONSOLE_OFF | SYSLOG_ACTION_CONSOLE_ON | SYSLOG_ACTION_CONSOLE_LEVEL => 0,
        SYSLOG_ACTION_READ => read_one(kernel_buf, kernel_len, false),
        SYSLOG_ACTION_READ_CLEAR => read_one(kernel_buf, kernel_len, true),
        SYSLOG_ACTION_READ_ALL => read_all(kernel_buf, kernel_len),
        _ => panic!("[klog] unknown syslog action: {action}"),
    }
}

fn read_one(buf : &mut [u8], len : usize, advance : bool) -> isize {
    // 格式化和可选游标推进必须在同一锁区间，避免记录刚借出便被其他 CPU 覆盖。
    let mut line = [0u8; KERNEL_LINE_MAX];
    KlogRingbuf::with(|ring| match ring.peek_next_unread() {
        Ok(view) => {
            let written = format_traditional(&view.meta, view.text, &mut line);
            if advance {
                ring.advance_read_cursor(view.meta.seq);
            }
            copy_out(buf, len, &line[..written]) as isize
        }
        Err(KlogError::NoUnread) => 0,
        Err(error) => panic!("[klog] peek_next_unread: {error:?}"),
    })
}

fn read_all(buf : &mut [u8], len : usize) -> isize {
    // `READ_ALL` 逐行消费；不保留半行游标，缓冲不足时该行已消费的尾部会被丢弃。
    let mut total = 0usize;
    while total < len {
        let mut line = [0u8; KERNEL_LINE_MAX];
        let Some(written) = KlogRingbuf::with(|ring| match ring.peek_next_unread() {
            Ok(view) => {
                let written = format_traditional(&view.meta, view.text, &mut line);
                ring.advance_read_cursor(view.meta.seq);
                Some(written)
            }
            Err(KlogError::NoUnread) => None,
            Err(error) => panic!("[klog] peek_next_unread: {error:?}"),
        }) else {
            break;
        };
        // 仅当整行能写入时继续读取；截断行已经推进游标，符合本实现的明确部分成功语义。
        let copied = written.min(len - total);
        buf[total..total + copied].copy_from_slice(&line[..copied]);
        total += copied;
        if copied < written {
            break;
        }
    }
    total as isize
}

/// `ABI:` WRITE priority 的高 3 位为 level，低 3 位为 facility。
///
/// `message_len` 已由 syscall 层用于建立安全内核切片，故这里以切片实际长度为准并返回该长度。
fn write_priority(priority : i32, message : &[u8], _message_len : usize) -> isize {
    let level = ((priority >> 3) & 7) as u8;
    let facility = (priority & 7) as u8;
    let mut meta = KlogRecordMeta::new(ts_nsec_now(),
                                       0,
                                       facility,
                                       level,
                                       KlogFlags::empty().with(KlogFlags::USER),
                                       caller_id_now());
    record_with_meta(&mut meta, message);
    message.len() as isize
}

/// 将内核生成行复制到已验证的内核缓冲。`len` 是 ABI 请求长度，可能小于切片容量。
fn copy_out(destination : &mut [u8], len : usize, source : &[u8]) -> usize {
    let copied = source.len().min(len);
    destination[..copied].copy_from_slice(&source[..copied]);
    copied
}
