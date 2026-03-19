use core::result::Result;
#[derive(Debug)]
pub enum FirmwareResetReason {
    NoReason,
    SystemFailure,
}
#[derive(Debug)]
pub enum FirmwareResetType {
    Shutdown,
    ColdReboot,
    WarmReboot,
}
#[derive(Debug)]
pub enum FirmwareResetError {
    Unsupported,
    Failed,
    Denied,
    Unavailable,
}

pub type FirmwareResetResult<T> = Result<T, FirmwareResetError>;

pub trait FirmwareReset {
    #[inline]
    fn is_available() -> bool {
        true
    }
    #[allow(unused_variables)]
    fn firmware_reset(
        reset_type: FirmwareResetType,
        reset_reason: FirmwareResetReason,
    ) -> FirmwareResetResult<()> {
        Err(FirmwareResetError::Unsupported)
    }
    #[inline]
    fn firmware_shutdown(reset_reason: FirmwareResetReason) -> FirmwareResetResult<()> {
        if !Self::is_available() {
            Err(FirmwareResetError::Unavailable)
        } else {
            Self::firmware_reset(FirmwareResetType::Shutdown, reset_reason)
        }
    }
    #[inline]
    fn firmware_reboot(reset_reason: FirmwareResetReason) -> FirmwareResetResult<()> {
        if !Self::is_available() {
            Err(FirmwareResetError::Unavailable)
        } else {
            Self::firmware_reset(FirmwareResetType::ColdReboot, reset_reason)
        }
    }
}
