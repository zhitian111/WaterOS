//! 单条内核消息的记录头（固定布局，供内核观测；不 `copy_to_user` 裸导出）。

use crate::KlogFlags;

/// syslog facility：内核（默认 boot 消息）。
pub const LOG_KERN: u8 = 0;
/// syslog facility：用户态。
pub const LOG_USER: u8 = 1;

/// syslog level：紧急。
pub const LOG_EMERG: u8 = 0;
/// syslog level：告警。
pub const LOG_ALERT: u8 = 1;
/// syslog level：严重。
pub const LOG_CRIT: u8 = 2;
/// syslog level：错误。
pub const LOG_ERR: u8 = 3;
/// syslog level：警告。
pub const LOG_WARNING: u8 = 4;
/// syslog level：通知。
pub const LOG_NOTICE: u8 = 5;
/// syslog level：信息。
pub const LOG_INFO: u8 = 6;
/// syslog level：调试。
pub const LOG_DEBUG: u8 = 7;

/// `DATA:` 一条 klog 记录的固定元数据（正文另存）。
///
/// `ABI:` 此布局用于内核内部观测；不是用户态可直接读取的 Linux ABI 结构。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KlogRecordMeta {
    /// 提交后由环分配的单调递增序号，用于定位读游标；服务重置后会重新开始计数。
    pub seq: u64,
    /// 记录时的单调时钟纳秒（由聚合层填入）；早期启动尚无时间源时为 0，不表示 Unix 时间。
    pub ts_nsec: u64,
    /// 实际保存的原始正文字节数，不含 traditional syslog 格式化时附加的前缀和换行。
    pub text_len: u16,
    /// syslog facility 分类值；当前写入口不校验范围，读取者应将未知值视作未分类。
    pub facility: u8,
    /// 内部标志（[`KlogFlags`] 编码为 `u8`）。
    pub flags: u8,
    /// syslog level（0–7）；传统格式化会掩码为低三位，故非法值不会越界但会丢失高位信息。
    pub level: u8,
    /// 写入时的内核任务 ID；早期启动、异常上下文或无调度上下文时为 0。
    pub caller_id: u32,
}

impl KlogRecordMeta {
    /// 构造未提交记录头（`seq` 由环实现填入）。
    #[inline]
    pub const fn new(
        ts_nsec: u64,
        text_len: u16,
        facility: u8,
        level: u8,
        flags: KlogFlags,
        caller_id: u32,
    ) -> Self {
        Self {
            seq: 0,
            ts_nsec,
            text_len,
            facility,
            flags: flags.0,
            level,
            caller_id,
        }
    }

    /// 解析标志位。
    #[inline]
    pub const fn klog_flags(self) -> KlogFlags {
        KlogFlags(self.flags)
    }

    /// 传统 syslog 读路径前缀用的单字符 level（`"0"`..`"7"`）。
    #[must_use]
    pub const fn traditional_level_char(self) -> u8 {
        b'0' + (self.level & 7)
    }
}
