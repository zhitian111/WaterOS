//! 系统复位占位：任务 05 接入真实板级复位控制器。

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
