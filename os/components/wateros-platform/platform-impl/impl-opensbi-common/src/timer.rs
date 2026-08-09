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
