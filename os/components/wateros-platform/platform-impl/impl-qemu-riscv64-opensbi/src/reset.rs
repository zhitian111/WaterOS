//! 本模块代码由AI完成
//! OpenSBI system reset 后端。

use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};
use sbi::{system_reset, ResetReason, ResetType};

/// 映射到 SBI 的复位类型枚举。
// 本结构代码由AI完成
pub enum OpenSBIResetType {
    /// 关机。
    Shutdown,
    /// 冷重启。
    ColdReboot,
    /// 热重启。
    WarmReboot,
}

/// 映射到 SBI 的复位原因枚举。
// 本结构代码由AI完成
pub enum OpenSBIResetReason {
    /// 无特定原因。
    NoReason,
    /// 系统故障。
    SystemFailure,
}

impl From<PlatformResetReason> for OpenSBIResetReason {
    #[inline]
    fn from(value: PlatformResetReason) -> Self {
        match value {
            PlatformResetReason::NoReason => Self::NoReason,
            PlatformResetReason::SystemFailure => Self::SystemFailure,
        }
    }
}

impl From<PlatformResetType> for OpenSBIResetType {
    #[inline]
    fn from(value: PlatformResetType) -> Self {
        match value {
            PlatformResetType::ColdReboot => Self::ColdReboot,
            PlatformResetType::Shutdown => Self::Shutdown,
            PlatformResetType::WarmReboot => Self::WarmReboot,
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

#[inline]
pub fn reset(reset_type: PlatformResetType,
             reset_reason: PlatformResetReason)
             -> PlatformResetResult<()> {
    system_reset(
        Into::<OpenSBIResetType>::into(reset_type),
        Into::<OpenSBIResetReason>::into(reset_reason),
    );
    Err(PlatformResetError::Failed)
}

#[inline]
pub fn reboot(reset_reason: PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::ColdReboot, reset_reason)
}

#[inline]
pub fn shutdown(reset_reason: PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::Shutdown, reset_reason)
}
