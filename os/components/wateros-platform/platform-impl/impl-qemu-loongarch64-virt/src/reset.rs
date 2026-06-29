//! QEMU LoongArch64 `virt` ACPI GED reset/shutdown 后端。

use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};

const VIRT_GED_REG_ADDR: usize = 0x100e_001c;
const ACPI_GED_REG_SLEEP_CTL: usize = 0x00;
const ACPI_GED_REG_RESET: usize = 0x02;
const ACPI_GED_RESET_VALUE: u8 = 0x42;
const ACPI_GED_SLP_TYP_S5: u8 = 0x05;
const ACPI_GED_SLP_EN: u8 = 0x20;
const ACPI_GED_SLP_TYP_SHIFT: u8 = 2;

/// 经 ACPI GED 寄存器请求关机或重启。
#[inline]
pub fn reset(reset_type: PlatformResetType,
             _reset_reason: PlatformResetReason)
             -> PlatformResetResult<()> {
    match reset_type {
        PlatformResetType::Shutdown => {
            let value = (ACPI_GED_SLP_TYP_S5 << ACPI_GED_SLP_TYP_SHIFT) | ACPI_GED_SLP_EN;
            unsafe {
                core::ptr::write_volatile((VIRT_GED_REG_ADDR + ACPI_GED_REG_SLEEP_CTL) as *mut u8,
                                          value);
            }
        }
        PlatformResetType::ColdReboot | PlatformResetType::WarmReboot => unsafe {
            core::ptr::write_volatile((VIRT_GED_REG_ADDR + ACPI_GED_REG_RESET) as *mut u8,
                                      ACPI_GED_RESET_VALUE);
        },
    }

    Err(PlatformResetError::Failed)
}

/// 冷重启快捷入口。
#[inline]
pub fn reboot(reset_reason: PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::ColdReboot, reset_reason)
}

/// 关机快捷入口。
#[inline]
pub fn shutdown(reset_reason: PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::Shutdown, reset_reason)
}
