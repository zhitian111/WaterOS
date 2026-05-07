//! 固件侧**绝对截止时间**定时器（典型映射为 SBI `set_timer`）；与 arch 的 tick 读数
//! 由上层组合为相对超时语义。

use core::result::Result;

/// 设置固件定时器失败的原因。
#[derive(Debug)]
pub enum FirmwareTimerError {
    Unsupported,
    Unavailable,
    InvalidDeadline,
    Failure,
}

/// 以与 arch `time` **相同刻度**表示的绝对截止时间（tick）；具体约定由平台 profile 定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FirmwareTimerDeadline(pub u64);

/// [`FirmwareTimerError`] 上的 `Result` 别名。
pub type FirmwareTimerResult<T> = Result<T, FirmwareTimerError>;

/// 固件定时器编程能力。
pub trait FirmwareTimer {
    #[inline]
    fn is_available() -> bool { true }
    #[inline]
    #[allow(unused_variables)]
    fn firmware_set_timer(time : FirmwareTimerDeadline) -> FirmwareTimerResult<()> {
        Err(FirmwareTimerError::Unsupported)
    }
}
