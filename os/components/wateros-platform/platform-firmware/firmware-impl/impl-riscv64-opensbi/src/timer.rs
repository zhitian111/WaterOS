//! SBI 定时器扩展：将绝对 tick deadline 交给固件。

use api_v0::timer::{
    FirmwareTimer, FirmwareTimerDeadline, FirmwareTimerError, FirmwareTimerResult,
};
use sbi::set_timer;

/// OpenSBI `set_timer` 封装。
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
