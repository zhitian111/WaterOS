//! 平台层时间频率：与 arch 的 `time` CSR 读数解耦，表示“内核调度与 tick 换算采用的 Hz”。

use core::result::Result;

/// 查询平台时间频率失败时的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformTimeError {
    /// 当前实现未支持或未配置频率来源。
    Unsupported,
    /// 依赖的固件或硬件能力当前不可用。
    Unavailable,
    /// 已配置但数值非法（例如为 0）。
    InvalidFrequency,
}

/// [`PlatformTimeError`] 上的 `Result` 别名。
pub type PlatformTimeResult<T> = Result<T, PlatformTimeError>;

/// 平台层时间能力：返回调度用的 **tick 频率（Hz）**。
///
/// 语义契约：该频率用于把 arch 的单调 tick 换算为 `Duration` 等；来源可以是 DTB、
/// 固件查询或板级常量。**不应**与“是否能在 arch 层读到 `timebase-frequency` CSR”
/// 混为一谈——后者属于 `wateros-platform-arch` 的职责与能力集。
pub trait PlatformTime {
    #[inline]
    fn time_frequency_hz() -> PlatformTimeResult<u64> {
        Err(PlatformTimeError::Unsupported)
    }
}

