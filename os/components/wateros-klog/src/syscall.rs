//! `sys_syslog` 语义（内核缓冲侧）；用户指针由 `wateros-syscall` 负责拷贝。

use api_v0::{
    is_write_priority, KlogError, KlogFlags, KlogRecordMeta, KlogStore, SYSLOG_ACTION_CLEAR,
    SYSLOG_ACTION_CLOSE, SYSLOG_ACTION_CONSOLE_LEVEL, SYSLOG_ACTION_CONSOLE_OFF,
    SYSLOG_ACTION_CONSOLE_ON, SYSLOG_ACTION_OPEN, SYSLOG_ACTION_READ, SYSLOG_ACTION_READ_ALL,
    SYSLOG_ACTION_READ_CLEAR, SYSLOG_ACTION_SIZE_BUFFER, SYSLOG_ACTION_SIZE_UNREAD,
};
use impl_ringbuf::KlogRingbuf;

use crate::export::format_traditional;
use crate::{caller_id_now, record_with_meta, ts_nsec_now};

const KERNEL_LINE_MAX: usize = 2048;

/// 处理 `sys_syslog` action；`user_buf` 为 syscall 层提供的内核侧缓冲。
pub fn dispatch_kernel(action: i32, user_buf: &mut [u8], user_len: usize) -> isize {
    if is_write_priority(action) {
        return write_priority(action, user_buf, user_len);
    }
    match action {
        SYSLOG_ACTION_CLOSE | SYSLOG_ACTION_OPEN => 0,
        SYSLOG_ACTION_SIZE_UNREAD => KlogRingbuf::with(|r| r.unread_bytes() as isize),
        SYSLOG_ACTION_SIZE_BUFFER => KlogRingbuf::with(|r| r.buffer_bytes() as isize),
        SYSLOG_ACTION_CLEAR => {
            KlogRingbuf::with(|r| r.clear_read_cursor());
            0
        }
        SYSLOG_ACTION_CONSOLE_OFF | SYSLOG_ACTION_CONSOLE_ON => 0,
        SYSLOG_ACTION_CONSOLE_LEVEL => 0,
        SYSLOG_ACTION_READ => read_one(user_buf, user_len, false),
        SYSLOG_ACTION_READ_CLEAR => read_one(user_buf, user_len, true),
        SYSLOG_ACTION_READ_ALL => read_all(user_buf, user_len),
        _ => panic!("[klog] unknown syslog action: {action}"),
    }
}

// READ / READ_CLEAR：取下一条未读，格式化为 traditional 行后拷贝到用户缓冲。
fn read_one(buf: &mut [u8], len: usize, advance: bool) -> isize {
    let mut line = [0u8; KERNEL_LINE_MAX];
    KlogRingbuf::with(|ring| match ring.peek_next_unread() {
        Ok(view) => {
            let n = format_traditional(&view.meta, view.text, &mut line);
            if advance {
                ring.advance_read_cursor(view.meta.seq);
            }
            copy_out(buf, len, &line[..n]) as isize
        }
        Err(KlogError::NoUnread) => 0,
        Err(e) => panic!("[klog] peek_next_unread: {e:?}"),
    })
}

fn read_all(buf: &mut [u8], len: usize) -> isize {
    let mut total = 0isize;
    let mut offset = 0usize;
    loop {
        if offset >= len {
            break;
        }
        let mut line = [0u8; KERNEL_LINE_MAX];
        let step = KlogRingbuf::with(|ring| match ring.peek_next_unread() {
            Ok(view) => {
                let n = format_traditional(&view.meta, view.text, &mut line);
                ring.advance_read_cursor(view.meta.seq);
                Some(n)
            }
            Err(KlogError::NoUnread) => None,
            Err(e) => panic!("[klog] peek_next_unread: {e:?}"),
        });
        let Some(n) = step else { break };
        let room = len.saturating_sub(offset);
        let copy_n = n.min(room);
        buf[offset..offset + copy_n].copy_from_slice(&line[..copy_n]);
        offset += copy_n;
        total += copy_n as isize;
        if copy_n < n {
            break;
        }
    }
    total
}

// WRITE：priority 高 3 位 level、低 3 位 facility（Linux 约定）。
fn write_priority(priority: i32, msg: &[u8], _msg_len: usize) -> isize {
    let level = ((priority >> 3) & 7) as u8;
    let facility = (priority & 7) as u8;
    let flags = KlogFlags::empty().with(KlogFlags::USER);
    let mut meta = KlogRecordMeta::new(
        ts_nsec_now(),
        0,
        facility,
        level,
        flags,
        caller_id_now(),
    );
    record_with_meta(&mut meta, msg);
    msg.len() as isize
}

#[inline]
fn copy_out(buf: &mut [u8], len: usize, src: &[u8]) -> isize {
    let n = src.len().min(len);
    buf[..n].copy_from_slice(&src[..n]);
    n as isize
}
