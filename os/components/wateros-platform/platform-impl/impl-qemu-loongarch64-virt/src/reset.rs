//! QEMU LoongArch64 `virt` 当前没有接入平台复位后端。

use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};

#[inline]
pub fn reset(_reset_type: PlatformResetType,
             _reset_reason: PlatformResetReason)
             -> PlatformResetResult<()> {
    Err(PlatformResetError::Unsupported)
}

#[inline]
pub fn reboot(reset_reason: PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::ColdReboot, reset_reason)
}

#[inline]
pub fn shutdown(reset_reason: PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::Shutdown, reset_reason)
}
