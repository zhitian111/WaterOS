//! 不可用的占位 deadline-timer 后端。

use api_v0::timer::{
    PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
};

#[inline]
pub fn set_timer(_ : PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
    Err(PlatformDeadlineTimerError::Unsupported)
}
