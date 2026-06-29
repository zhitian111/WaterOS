//! 平台 deadline 定时器能力：把绝对 tick deadline 编程到当前平台后端。

use core::result::Result;

/// 设置平台 deadline 定时器失败时的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformDeadlineTimerError {
    /// 当前平台没有可用 deadline timer。
    Unsupported,
    /// 定时器后端暂不可用。
    Unavailable,
    /// 后端调用失败。
    Failure,
    /// deadline 无法由当前后端表示。
    InvalidDeadline,
}

/// 绝对 tick deadline；tick 源需与 `platform::timer::now_tick()` 同源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlatformTimerDeadline(
    /// 绝对 tick deadline，与 `platform::timer::now_tick()` 同源。
    pub u64,
);

/// [`PlatformDeadlineTimerError`] 上的 `Result` 别名。
pub type PlatformDeadlineTimerResult<T> = Result<T, PlatformDeadlineTimerError>;

/// 平台 deadline timer 能力。
pub trait PlatformDeadlineTimer {
    #[inline]
    fn platform_set_timer(_time : PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
        Err(PlatformDeadlineTimerError::Unsupported)
    }

    #[inline]
    fn is_available() -> bool { true }
}
