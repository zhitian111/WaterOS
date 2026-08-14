//! 平台 deadline timer 占位：任务 05 经 OpenSBI timer 扩展实现。

use api_v0::timer::{
    PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
};

pub fn set_timer(_time : PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
    Err(PlatformDeadlineTimerError::Unsupported)
}
