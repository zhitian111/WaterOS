//! 记录标志位（内核私有语义，不导出给用户态裸结构）。

/// [`KlogRecordMeta::flags`] 位域。
///
/// 未知位必须原样保留，便于后续内核实现扩展；消费者只可用 [`Self::contains`] 判断自己理解的位。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct KlogFlags(pub u8);

impl KlogFlags {
    /// 本记录是上一行的续行（对应 Linux continuation），消费者可据此选择不另起一行显示。
    pub const CONT: u8 = 1 << 0;
    /// 正文超过单记录存储上限而被截断；环覆盖整条旧记录不使用此标志。
    pub const TRUNC: u8 = 1 << 1;
    /// 来自用户态 `sys_syslog` WRITE，供审计或过滤策略与内核自身日志区分来源。
    pub const USER: u8 = 1 << 2;

    /// 新建标志集。
    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// 是否包含 `flag` 位。
    #[inline]
    pub const fn contains(self, flag: u8) -> bool {
        (self.0 & flag) != 0
    }

    /// 设置 `flag` 位。
    #[inline]
    pub const fn with(self, flag: u8) -> Self {
        Self(self.0 | flag)
    }
}
