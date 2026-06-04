//! Linux `syslog(2)` / `klogctl` action 常量（与 man page 对齐）。

/// `SYSLOG_ACTION_CLOSE`
pub const SYSLOG_ACTION_CLOSE: i32 = 0;
/// `SYSLOG_ACTION_OPEN`
pub const SYSLOG_ACTION_OPEN: i32 = 1;
/// `SYSLOG_ACTION_READ`
pub const SYSLOG_ACTION_READ: i32 = 2;
/// `SYSLOG_ACTION_READ_ALL`
pub const SYSLOG_ACTION_READ_ALL: i32 = 3;
/// `SYSLOG_ACTION_READ_CLEAR`
pub const SYSLOG_ACTION_READ_CLEAR: i32 = 4;
/// `SYSLOG_ACTION_CLEAR`
pub const SYSLOG_ACTION_CLEAR: i32 = 5;
/// `SYSLOG_ACTION_CONSOLE_OFF`
pub const SYSLOG_ACTION_CONSOLE_OFF: i32 = 6;
/// `SYSLOG_ACTION_CONSOLE_ON`
pub const SYSLOG_ACTION_CONSOLE_ON: i32 = 7;
/// `SYSLOG_ACTION_CONSOLE_LEVEL`
pub const SYSLOG_ACTION_CONSOLE_LEVEL: i32 = 8;
/// `SYSLOG_ACTION_SIZE_UNREAD`
pub const SYSLOG_ACTION_SIZE_UNREAD: i32 = 9;
/// `SYSLOG_ACTION_SIZE_BUFFER`
pub const SYSLOG_ACTION_SIZE_BUFFER: i32 = 10;

/// 将原始 `type` 解码为已知 action；未知返回 `None`。
#[inline]
pub fn decode_action(raw: i32) -> Option<i32> {
    match raw {
        SYSLOG_ACTION_CLOSE
        | SYSLOG_ACTION_OPEN
        | SYSLOG_ACTION_READ
        | SYSLOG_ACTION_READ_ALL
        | SYSLOG_ACTION_READ_CLEAR
        | SYSLOG_ACTION_CLEAR
        | SYSLOG_ACTION_CONSOLE_OFF
        | SYSLOG_ACTION_CONSOLE_ON
        | SYSLOG_ACTION_CONSOLE_LEVEL
        | SYSLOG_ACTION_SIZE_UNREAD
        | SYSLOG_ACTION_SIZE_BUFFER => Some(raw),
        _ => None,
    }
}

/// `type` 为 WRITE 优先级（非 0..=10 action）时返回 true。
#[inline]
pub fn is_write_priority(raw: i32) -> bool {
    decode_action(raw).is_none() && raw != 0
}
