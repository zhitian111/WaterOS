use core::result::Result;
#[derive(Debug)]
pub enum FirmwareTimerError {
    Unsupported,
    Unavailable,
    InvalidDeadline,
    Failure,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FirmwareTimerDeadline(pub u64);
pub type FirmwareTimerResult<T> = Result<T, FirmwareTimerError>;
pub trait FirmwareTimer {
    #[inline]
    fn is_available() -> bool { true }
    #[inline]
    #[allow(unused_variables)]
    fn firmware_set_timer(time : FirmwareTimerDeadline) -> FirmwareTimerResult<()> {
        Err(FirmwareTimerError::Unsupported)
    }
}
