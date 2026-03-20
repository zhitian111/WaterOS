use api_v0::reset::{
    FirmwareReset, FirmwareResetError, FirmwareResetReason, FirmwareResetResult, FirmwareResetType,
};
use sbi::{system_reset, ResetReason, ResetType};
pub struct OpenSBIReset;
pub enum OpenSBIResetType {
    Shutdown,
    ColdReboot,
    WarmReboot,
}
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
