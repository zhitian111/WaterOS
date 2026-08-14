//! 系统复位占位：任务 09 接入 PM1/PM controller（loongson,ls2k1000-pmc）。

use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};

pub fn reset(_reset_type : PlatformResetType,
             _reset_reason : PlatformResetReason)
             -> PlatformResetResult<()> {
    Err(PlatformResetError::Unsupported)
}

pub fn reboot(_reset_reason : PlatformResetReason) -> PlatformResetResult<()> {
    Err(PlatformResetError::Unsupported)
}

pub fn shutdown(_reset_reason : PlatformResetReason) -> PlatformResetResult<()> {
    Err(PlatformResetError::Unsupported)
}
