//! OpenSBI system reset 后端。
//!
//! PLATFORM_BOUNDARY: 复位请求经 SBI System Reset 扩展离开 S-mode；该函数若返回，
//! 表示固件拒绝或未实现请求，而不是已经完成复位。

use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};
use sbi::{system_reset, ResetReason, ResetType};

/// WaterOS reset 类型到 SBI raw reset type 的窄映射。
pub enum OpenSBIResetType {
    /// 关机。
    Shutdown,
    /// 冷重启。
    ColdReboot,
    /// 热重启。
    WarmReboot,
}

/// WaterOS reset 原因到 SBI raw reset reason 的窄映射。
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
/// 向 OpenSBI 发起系统 reset。
///
/// 调用成功时 firmware 不应返回；因此返回路径统一映射为 `Failed`，保留给调用方的
/// panic/降级逻辑处理。不得在持锁或中断上下文中依赖该调用返回。
pub fn reset(
    reset_type: PlatformResetType,
    reset_reason: PlatformResetReason,
) -> PlatformResetResult<()> {
    system_reset(
        Into::<OpenSBIResetType>::into(reset_type),
        Into::<OpenSBIResetReason>::into(reset_reason),
    );
    Err(PlatformResetError::Failed)
}

#[inline]
/// 请求冷重启的便捷入口。
pub fn reboot(reset_reason: PlatformResetReason) -> PlatformResetResult<()> {
    reset(
        PlatformResetType::ColdReboot,
        reset_reason,
    )
}

#[inline]
/// 请求关机的便捷入口。
pub fn shutdown(reset_reason: PlatformResetReason) -> PlatformResetResult<()> {
    reset(
        PlatformResetType::Shutdown,
        reset_reason,
    )
}
