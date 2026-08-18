//! Linux `syslog(2)` / `klogctl` action 常量（与 man page 对齐）。

/// `SYSLOG_ACTION_CLOSE`
/// 结束一次传统 syslog 读取会话。当前实现没有每调用者会话状态，因此它是成功但无副作用的兼容操作。
pub const SYSLOG_ACTION_CLOSE: i32 = 0;
/// `SYSLOG_ACTION_OPEN`
/// 开始一次传统 syslog 读取会话；与 `CLOSE` 一样仅保留 Linux ABI 兼容性。
pub const SYSLOG_ACTION_OPEN: i32 = 1;
/// `SYSLOG_ACTION_READ`
/// 读取当前读游标指向的一条记录，但不推进游标；重复调用会获得同一条记录。
pub const SYSLOG_ACTION_READ: i32 = 2;
/// `SYSLOG_ACTION_READ_ALL`
/// 从全局读游标连续读取多条记录，并推进游标；输出缓冲放不下下一条完整行时停止。
pub const SYSLOG_ACTION_READ_ALL: i32 = 3;
/// `SYSLOG_ACTION_READ_CLEAR`
/// 读取一条记录并将游标推进到该记录之后，因此同一记录不会再被后续读取返回。
pub const SYSLOG_ACTION_READ_CLEAR: i32 = 4;
/// `SYSLOG_ACTION_CLEAR`
/// 将读游标移到最新记录之后；记录仍留在环中，后续追加的记录仍可读取。
pub const SYSLOG_ACTION_CLEAR: i32 = 5;
/// `SYSLOG_ACTION_CONSOLE_OFF`
/// 请求关闭控制台日志输出。当前 klog 仅维护缓冲区，动作成功但不会改变 console 配置。
pub const SYSLOG_ACTION_CONSOLE_OFF: i32 = 6;
/// `SYSLOG_ACTION_CONSOLE_ON`
/// 请求开启控制台日志输出。当前实现保留该 ABI 编号但不拥有 console 策略。
pub const SYSLOG_ACTION_CONSOLE_ON: i32 = 7;
/// `SYSLOG_ACTION_CONSOLE_LEVEL`
/// 设置控制台日志级别。该实现目前忽略该值，避免 syslog 缓冲接口反向控制运行时日志层。
pub const SYSLOG_ACTION_CONSOLE_LEVEL: i32 = 8;
/// `SYSLOG_ACTION_SIZE_UNREAD`
/// 查询读游标之后所有正文的近似字节数；并发追加或覆盖后该值可能立即过期。
pub const SYSLOG_ACTION_SIZE_UNREAD: i32 = 9;
/// `SYSLOG_ACTION_SIZE_BUFFER`
/// 查询日志正文环的配置容量，单位为字节，而不是当前已提交记录的实际占用量。
pub const SYSLOG_ACTION_SIZE_BUFFER: i32 = 10;

/// 将原始 `type` 解码为已知 action；未知返回 `None`。
///
/// `ABI:` Linux `syslog(2)` 把非 action 的整数解释为 WRITE priority，调用方须再用
/// [`is_write_priority`] 区分。
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
