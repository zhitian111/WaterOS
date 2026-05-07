//! SBI 系统复位扩展：将关机/重启请求映射为 `system_reset`。
//!
//! 当前实现：调用后仍返回 `Err(Failed)`，因成功路径通常不返回；调用方应视具体
//! 策略处理（文档化“当前行为”）。

use api_v0::reset::{
    FirmwareReset, FirmwareResetError, FirmwareResetReason, FirmwareResetResult, FirmwareResetType,
};
use sbi::{system_reset, ResetReason, ResetType};

/// OpenSBI 复位后端。
pub struct OpenSBIReset;

/// 映射到 SBI 的复位类型枚举（实现细节，非 `firmware-api` 公共面）。
pub enum OpenSBIResetType {
    Shutdown,
    ColdReboot,
    WarmReboot,
}

/// 映射到 SBI 的复位原因枚举。
pub enum OpenSBIResetReason {
    NoReason,
    SystemFailure,
}
impl From<FirmwareResetReason> for OpenSBIResetReason {
    #[inline]
    fn from(value : FirmwareResetReason) -> Self {
        match value {
            FirmwareResetReason::NoReason => Self::NoReason,
            FirmwareResetReason::SystemFailure => Self::SystemFailure,
        }
    }
}
impl From<FirmwareResetType> for OpenSBIResetType {
    #[inline]
    fn from(value : FirmwareResetType) -> Self {
        match value {
            FirmwareResetType::ColdReboot => Self::ColdReboot,
            FirmwareResetType::Shutdown => Self::Shutdown,
            FirmwareResetType::WarmReboot => Self::WarmReboot,
        }
    }
}
impl ResetReason for OpenSBIResetReason {
    #[inline]
    fn raw(&self) -> u32 {
        match self {
            Self::NoReason => sbi::NoReason.raw(),
            Self::SystemFailure => sbi::SystemFailure.raw(),
        }
    }
}
impl ResetType for OpenSBIResetType {
    #[inline]
    fn raw(&self) -> u32 {
        match self {
            Self::WarmReboot => sbi::WarmReboot.raw(),
            Self::Shutdown => sbi::Shutdown.raw(),
            Self::ColdReboot => sbi::ColdReboot.raw(),
        }
    }
}
impl FirmwareReset for OpenSBIReset {
    #[inline]
    fn firmware_reset(reset_type : FirmwareResetType,
                      reset_reason : FirmwareResetReason)
                      -> FirmwareResetResult<()> {
        system_reset(Into::<OpenSBIResetType>::into(reset_type),
                     Into::<OpenSBIResetReason>::into(reset_reason));
        // unreachable!("System reset failure !");
        Err(FirmwareResetError::Failed)
    }
}
