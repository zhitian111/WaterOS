//! VirtIO PCI ISR capability discovery for platform INTx routing.
use api_v0::{DriverError, DriverResult};
pub use virtio_drivers::transport::pci::bus::DeviceFunction;
use virtio_drivers::transport::pci::bus::{Cam, ConfigurationAccess, MmioCam};

/// Find the mapped ISR byte and interrupt pin after BAR assignment.
/// # Safety
/// `config_base` must map the ECAM window for `df`; the device's BARs must remain mapped.
pub unsafe fn find_isr(config_base : usize, df : DeviceFunction) -> DriverResult<(usize, u8)> {
    let config = unsafe { MmioCam::new(config_base as *mut u8, Cam::Ecam) };
    let pin = ((config.read_word(df, 0x3C) >> 8) & 0xFF) as u8;
    if !(1..=4).contains(&pin) {
        return Err(DriverError::Unsupported);
    }
    let mut offset = (config.read_word(df, 0x34) & 0xFC) as u8;
    // Conventional PCI capability space contains at most 48 aligned headers; reject cycles.
    for _ in 0..48 {
        if offset < 0x40 || offset > 0xF0 || offset & 3 != 0 {
            break;
        }
        let header = config.read_word(df, offset);
        if header & 0xFF == 9 && header >> 24 == 3 && (header >> 16) & 0xFF >= 16 {
            let bar = (config.read_word(df, offset + 4) & 0xFF) as u8;
            if bar >= 6 {
                return Err(DriverError::InvalidParam);
            }
            let low = config.read_word(df, 0x10 + 4 * bar);
            if low & 1 != 0 {
                return Err(DriverError::Unsupported);
            }
            let mut base = (low & !15) as u64;
            if low & 6 == 4 {
                if bar >= 5 {
                    return Err(DriverError::InvalidParam);
                }
                base |= (config.read_word(df, 0x14 + 4 * bar) as u64) << 32;
            }
            let displacement = config.read_word(df, offset + 8) as u64;
            let length = config.read_word(df, offset + 12);
            if base == 0 || length < 1 {
                return Err(DriverError::InvalidParam);
            }
            let address = base.checked_add(displacement)
                              .and_then(|v| usize::try_from(v).ok())
                              .ok_or(DriverError::InvalidParam)?;
            return Ok((address, pin));
        }
        offset = ((header >> 8) & 0xFF) as u8;
    }
    Err(DriverError::Unsupported)
}

/// Enable legacy INTx after the device's acknowledgement endpoint is registered.
/// # Safety
/// `config_base` must map the ECAM window containing the live `df`.
pub unsafe fn enable_intx(config_base : usize, df : DeviceFunction) {
    let mut config = unsafe { MmioCam::new(config_base as *mut u8, Cam::Ecam) };
    // The upper halfword is PCI status (write-one-to-clear); write zero there.
    let command = config.read_word(df, 4) & 0xFFFF & !(1 << 10);
    config.write_word(df, 4, command);
}
