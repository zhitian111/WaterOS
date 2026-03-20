use api_v0::timer::{
    FirmwareTimer, FirmwareTimerDeadline, FirmwareTimerError, FirmwareTimerResult,
};
use sbi::set_timer;
pub struct OpenSBITimer;
impl FirmwareTimer for OpenSBITimer {
    #[inline]
    fn firmware_set_timer(time : FirmwareTimerDeadline) -> FirmwareTimerResult<()> {
        if set_timer(time.0).is_ok() {
            Ok(())
        } else {
            Err(FirmwareTimerError::Failure)
        }
    }
}
