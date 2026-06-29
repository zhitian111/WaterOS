//! 本模块代码由AI完成
//! OpenSBI timer 后端：将绝对 tick deadline 交给 SBI `set_timer`。

use api_v0::timer::{
    PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
};
use sbi::set_timer as sbi_set_timer;

/// 设置下一次定时器中断 deadline。
#[inline]
pub fn set_timer(time: PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
    if sbi_set_timer(time.0).is_ok() {
        Ok(())
    } else {
        Err(PlatformDeadlineTimerError::Failure)
    }
}
