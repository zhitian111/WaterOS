//! 平台复位能力：关机、冷/热重启等请求。

use core::result::Result;

/// 平台复位原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformResetReason {
    /// 无特定原因。
    NoReason,
    /// 系统故障。
    SystemFailure,
}

/// 平台复位类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformResetType {
    /// 关机。
    Shutdown,
    /// 冷重启。
    ColdReboot,
    /// 热重启。
    WarmReboot,
}

/// 平台复位失败时的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformResetError {
    /// 当前平台不支持该复位类型。
    Unsupported,
    /// 复位后端暂不可用。
    Unavailable,
    /// 后端调用失败。
    Failed,
}

/// [`PlatformResetError`] 上的 `Result` 别名。
pub type PlatformResetResult<T> = Result<T, PlatformResetError>;
