//! OpenSBI timer 后端：将绝对 tick deadline 交给 SBI `set_timer`。
//!
//! TIME_CONTRACT: `PlatformTimerDeadline` 与 RISC-V `time` CSR 同源。这里不计算
//! duration、不维护全局 tick，也不打开 `sie.STIE`；后两者分别属于聚合层和 arch。

use api_v0::timer::{
    PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
};
use sbi::set_timer as sbi_set_timer;

/// 设置本 hart 的下一次 timer deadline。
///
/// SBI 只接收绝对 time 值；若 deadline 已过，具体固件通常会尽快产生中断，调用方
/// 不能把该行为当作错误或依赖其精确延迟。
#[inline]
pub fn set_timer(time: PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
    if sbi_set_timer(time.0).is_ok() {
        Ok(())
    } else {
        Err(PlatformDeadlineTimerError::Failure)
    }
}
