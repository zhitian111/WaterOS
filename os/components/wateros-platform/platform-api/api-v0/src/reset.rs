//! 平台复位能力：关机、冷/热重启等请求。

use core::result::Result;

/// 平台复位原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformResetReason {
    NoReason,
    SystemFailure,
}

/// 平台复位类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformResetType {
    Shutdown,
    ColdReboot,
    WarmReboot,
}

/// 平台复位失败时的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformResetError {
    Unsupported,
    Unavailable,
    Failed,
}

/// [`PlatformResetError`] 上的 `Result` 别名。
pub type PlatformResetResult<T> = Result<T, PlatformResetError>;

/// 平台复位能力。
pub trait PlatformReset {
    #[inline]
    fn platform_reset(reset_type : PlatformResetType,
                      reset_reason : PlatformResetReason)
                      -> PlatformResetResult<()> {
        let _ = (reset_type, reset_reason);
        Err(PlatformResetError::Unsupported)
    }

    #[inline]
    fn platform_shutdown(reset_reason : PlatformResetReason) -> PlatformResetResult<()> {
        if !Self::is_available() {
            Err(PlatformResetError::Unavailable)
        } else {
            Self::platform_reset(PlatformResetType::Shutdown,
                                 reset_reason)
        }
    }

    #[inline]
    fn platform_reboot(reset_reason : PlatformResetReason) -> PlatformResetResult<()> {
        if !Self::is_available() {
            Err(PlatformResetError::Unavailable)
        } else {
            Self::platform_reset(PlatformResetType::ColdReboot,
                                 reset_reason)
        }
    }

    #[inline]
    fn is_available() -> bool { true }
}
