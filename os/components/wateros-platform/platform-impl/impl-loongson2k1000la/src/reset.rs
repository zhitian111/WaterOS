use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};

/// TODO(real-hardware): implement the PM/reset-controller sequence after board validation.
pub fn reset(_ : PlatformResetType, _ : PlatformResetReason) -> PlatformResetResult<()> {
    Err(PlatformResetError::Unsupported)
}
pub fn reboot(reason : PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::ColdReboot, reason)
}
pub fn shutdown(reason : PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::Shutdown, reason)
}
