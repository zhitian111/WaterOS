use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};

/// TODO(real-hardware): implement the PM/reset-controller sequence after board validation.
pub fn reset(_ : PlatformResetType, _ : PlatformResetReason) -> PlatformResetResult<()> {
    Err(PlatformResetError::Unsupported)
}
pub fn reboot(reason : PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::ColdReboot, reason)
}
pub fn shutdown(reason : PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::Shutdown, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reset_request_remains_explicitly_unsupported_until_board_validation() {
        for reset_type in [PlatformResetType::Shutdown,
                           PlatformResetType::ColdReboot,
                           PlatformResetType::WarmReboot]
        {
            assert_eq!(reset(reset_type, PlatformResetReason::NoReason),
                       Err(PlatformResetError::Unsupported));
            assert_eq!(reset(reset_type, PlatformResetReason::SystemFailure),
                       Err(PlatformResetError::Unsupported));
        }
    }
}
