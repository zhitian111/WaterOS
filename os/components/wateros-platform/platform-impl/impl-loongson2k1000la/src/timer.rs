//! 平台 deadline timer 占位：任务 09 经 CSR TCFG/TICLR 实现。

use api_v0::timer::{
    PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
};

pub fn set_timer(_time : PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
    Err(PlatformDeadlineTimerError::Unsupported)
}
