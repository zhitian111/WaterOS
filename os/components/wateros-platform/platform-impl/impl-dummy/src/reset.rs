//! 不可用的占位 reset 后端。

use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};

#[inline]
pub fn reset(_ : PlatformResetType, _ : PlatformResetReason) -> PlatformResetResult<()> {
    Err(PlatformResetError::Unsupported)
}

#[inline]
pub fn reboot(reason : PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::ColdReboot, reason)
}

#[inline]
pub fn shutdown(reason : PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::Shutdown, reason)
}
