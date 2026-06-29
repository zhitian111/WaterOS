//! 记录标志位（内核私有语义，不导出给用户态裸结构）。
//! 本模块代码由AI完成

/// [`KlogRecordMeta::flags`] 位域。
// 本结构代码由AI完成
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct KlogFlags(pub u8);

impl KlogFlags {
    /// 续行（对应 Linux continuation）。
    pub const CONT: u8 = 1 << 0;
    /// 正文因上限或环压力被截断。
    pub const TRUNC: u8 = 1 << 1;
    /// 来自用户态 `sys_syslog` WRITE。
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
