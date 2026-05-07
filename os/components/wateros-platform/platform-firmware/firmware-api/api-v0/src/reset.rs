//! 经固件请求关机或重启；**不**描述 MMIO 看门狗等板级外设复位。

use core::result::Result;

/// 复位原因提示（映射到 SBI `system_reset` 的 reason 域等）。
#[derive(Debug)]
pub enum FirmwareResetReason {
    NoReason,
    SystemFailure,
}

/// 请求的复位类型（关机、冷启动、热启动等）。
#[derive(Debug)]
pub enum FirmwareResetType {
    Shutdown,
    ColdReboot,
    WarmReboot,
}

/// 固件拒绝或未能执行复位时的错误。
#[derive(Debug)]
pub enum FirmwareResetError {
    Unsupported,
    Failed,
    Denied,
    Unavailable,
}

/// [`FirmwareResetError`] 上的 `Result` 别名。
pub type FirmwareResetResult<T> = Result<T, FirmwareResetError>;

/// 固件系统复位能力。
pub trait FirmwareReset {
    #[inline]
    fn is_available() -> bool { true }
    #[allow(unused_variables)]
    fn firmware_reset(reset_type : FirmwareResetType,
                      reset_reason : FirmwareResetReason)
                      -> FirmwareResetResult<()> {
        Err(FirmwareResetError::Unsupported)
    }
    #[inline]
    fn firmware_shutdown(reset_reason : FirmwareResetReason) -> FirmwareResetResult<()> {
        if !Self::is_available() {
            Err(FirmwareResetError::Unavailable)
        } else {
            Self::firmware_reset(FirmwareResetType::Shutdown,
                                 reset_reason)
        }
    }
    #[inline]
    fn firmware_reboot(reset_reason : FirmwareResetReason) -> FirmwareResetResult<()> {
        if !Self::is_available() {
            Err(FirmwareResetError::Unavailable)
        } else {
            Self::firmware_reset(FirmwareResetType::ColdReboot,
                                 reset_reason)
        }
    }
}
