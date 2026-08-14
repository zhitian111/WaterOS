//! OpenSBI system reset 后端。
//!
//! 调用成功时固件不应返回；返回路径统一映射为 `Failed`，保留给调用方的降级逻辑。

use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};
use sbi::{ResetReason, ResetType};

struct ResetTypeValue(PlatformResetType);
struct ResetReasonValue(PlatformResetReason);

impl ResetType for ResetTypeValue {
    fn raw(&self) -> u32 {
        match self.0 {
            PlatformResetType::Shutdown => sbi::Shutdown.raw(),
            PlatformResetType::ColdReboot => sbi::ColdReboot.raw(),
            PlatformResetType::WarmReboot => sbi::WarmReboot.raw(),
        }
    }
}

impl ResetReason for ResetReasonValue {
    fn raw(&self) -> u32 {
        match self.0 {
            PlatformResetReason::NoReason => sbi::NoReason.raw(),
            PlatformResetReason::SystemFailure => sbi::SystemFailure.raw(),
        }
    }
}

pub fn reset(reset_type : PlatformResetType,
             reset_reason : PlatformResetReason)
             -> PlatformResetResult<()> {
    sbi::system_reset(ResetTypeValue(reset_type),
                      ResetReasonValue(reset_reason));
    Err(PlatformResetError::Failed)
}

pub fn reboot(reason : PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::ColdReboot, reason)
}

pub fn shutdown(reason : PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::Shutdown, reason)
}
