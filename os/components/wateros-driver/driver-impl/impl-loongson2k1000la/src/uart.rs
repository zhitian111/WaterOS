//! DTB-driven NS16550 registration for the Loongson 2K1000LA profile.
//!
//! The implementation is shared with QEMU/VisionFive through
//! `impl-uart-16550`; this module only translates the DTB `reg-shift` field.

use api_v0::{DriverError, DriverResult};
use character::{register_uart_character_device, RegisterLayout};

use crate::topology::UartDescription;

fn layout_for_shift(register_shift : u32) -> DriverResult<RegisterLayout> {
    match register_shift {
        0 => Ok(RegisterLayout::Byte16550),
        2 => Ok(RegisterLayout::DwApb32),
        _ => Err(DriverError::InvalidDtb),
    }
}

/// Register all DTB-described UARTs in the character-device registry.
///
/// # Safety
/// Every UART MMIO region must already be identity/device mapped and exclusive
/// to this call. The shared driver writes IER during initialization.
///
/// `UNVERIFIED_ON_HARDWARE`: register layout and live UART electrical behavior
/// are inferred from DTB/Linux conventions and require board validation.
pub unsafe fn register_from_topology(uarts : &[UartDescription]) -> DriverResult<usize> {
    let mut registered = 0usize;
    for uart in uarts {
        let layout = layout_for_shift(uart.register_shift)?;
        register_uart_character_device(uart.mmio.base, layout);
        registered = registered.checked_add(1).ok_or(DriverError::InvalidParam)?;
    }
    Ok(registered)
}

#[cfg(test)]
mod tests {
    use super::layout_for_shift;
    use character::RegisterLayout;

    #[test]
    fn accepts_documented_register_layouts_without_claiming_hardware_proof() {
        // Do not invoke registration in host tests: it performs volatile MMIO.
        assert_eq!(layout_for_shift(0), Ok(RegisterLayout::Byte16550));
        assert_eq!(layout_for_shift(2), Ok(RegisterLayout::DwApb32));
        assert_eq!(layout_for_shift(1), Err(api_v0::DriverError::InvalidDtb));
    }
}
