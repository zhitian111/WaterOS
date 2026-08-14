//! OpenSBI timer 后端：将绝对 tick deadline 交给 SBI `set_timer`。

use api_v0::timer::{
    PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
};

pub fn set_timer(time : PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
    if sbi::set_timer(time.0).is_ok() {
        Ok(())
    } else {
        Err(PlatformDeadlineTimerError::Failure)
    }
}
