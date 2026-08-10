//! Read-only PCIe ECAM probe primitives for the 2K1000LA bring-up.
//!
//! The real ECAM reader is intentionally not provided yet: its DTB window,
//! mapping attributes and access ordering require board evidence. This module
//! only validates addresses and interprets configuration-space snapshots.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciProbeError {
    InvalidRegister,
    AddressOverflow,
    InvalidWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciLocation {
    pub bus : u8,
    pub device : u8,
    pub function : u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciIdentity {
    pub location : PciLocation,
    pub vendor_id : u16,
    pub device_id : u16,
    pub class_code : u8,
    pub subclass : u8,
    pub prog_if : u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciProbeResult {
    Absent,
    Present(PciIdentity),
}

/// Return the ECAM byte offset for one aligned config-space register.
pub fn ecam_offset(location : PciLocation, register : u16) -> Result<u64, PciProbeError> {
    if register >= 0x1000 || register % 4 != 0 {
        return Err(PciProbeError::InvalidRegister);
    }
    Ok(((location.bus as u64) << 20) |
       ((location.device as u64) << 15) |
       ((location.function as u64) << 12) |
       register as u64)
}

/// Combine a DTB-provided ECAM base with an offset without wrapping.
pub fn ecam_address(base : usize,
                    location : PciLocation,
                    register : u16)
                    -> Result<usize, PciProbeError> {
    let offset = ecam_offset(location, register)? as usize;
    base.checked_add(offset).ok_or(PciProbeError::AddressOverflow)
}

pub trait ConfigReader {
    fn read32(&self, offset : u64) -> u32;
}

/// Read-only ECAM window backed by device memory.
///
/// The caller must provide an already mapped, exclusively readable DTB window.
/// This type never writes PCI configuration space. Physical ECAM mapping and
/// access-fault behavior remain `UNVERIFIED_ON_HARDWARE` for 2K1000LA.
pub struct VolatileConfigReader {
    base : *const u8,
    size : usize,
}

unsafe impl Send for VolatileConfigReader {}

impl VolatileConfigReader {
    /// # Safety
    /// `region` must be a valid, readable device-memory mapping for the
    /// lifetime of the reader; no other owner may revoke it concurrently.
    pub unsafe fn from_region(region : api_v0::MmioRegion) -> Result<Self, PciProbeError> {
        if region.base == 0 || region.base % 4096 != 0 || region.size < 4 {
            return Err(PciProbeError::InvalidWindow);
        }
        Ok(Self { base : region.base as *const u8,
                  size : region.size })
    }

    fn read_window(&self, offset : u64) -> u32 {
        let offset = usize::try_from(offset).ok();
        let Some(offset) = offset else { return u32::MAX; };
        if offset % 4 != 0 || offset.checked_add(4).is_none_or(|end| end > self.size) {
            return u32::MAX;
        }
        // SAFETY: construction requires the caller to provide a mapped,
        // readable device window; the bounds check above proves the access.
        unsafe { core::ptr::read_volatile(self.base.add(offset).cast::<u32>()) }
    }
}

impl ConfigReader for VolatileConfigReader {
    fn read32(&self, offset : u64) -> u32 { self.read_window(offset) }
}

pub fn probe_volatile(reader : &VolatileConfigReader,
                      location : PciLocation)
                      -> PciProbeResult {
    probe(reader, location)
}

/// Interpret vendor/device and class-code registers without writing config.
pub fn probe<R : ConfigReader>(reader : &R,
                               location : PciLocation)
                               -> PciProbeResult {
    let vendor_device = reader.read32(ecam_offset(location, 0).unwrap());
    let vendor_id = vendor_device as u16;
    if vendor_id == 0xffff { return PciProbeResult::Absent; }
    let class_register = reader.read32(ecam_offset(location, 8).unwrap());
    PciProbeResult::Present(PciIdentity { location,
                                          vendor_id,
                                          device_id : (vendor_device >> 16) as u16,
                                          prog_if : (class_register >> 8) as u8,
                                          subclass : (class_register >> 16) as u8,
                                          class_code : (class_register >> 24) as u8 })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture { vendor_device : u32, class_register : u32 }
    impl ConfigReader for Fixture {
        fn read32(&self, offset : u64) -> u32 {
            match offset & 0xfff { 0 => self.vendor_device, 8 => self.class_register, _ => 0 }
        }
    }

    #[test]
    fn ecam_encoding_matches_pci_layout() {
        assert_eq!(ecam_offset(PciLocation { bus : 2, device : 3, function : 1 }, 0x40),
                   Ok((2u64 << 20) | (3u64 << 15) | (1u64 << 12) | 0x40));
    }

    #[test]
    fn rejects_unaligned_or_out_of_range_registers() {
        let location = PciLocation { bus : 0, device : 0, function : 0 };
        assert_eq!(ecam_offset(location, 2), Err(PciProbeError::InvalidRegister));
        assert_eq!(ecam_offset(location, 0x1000), Err(PciProbeError::InvalidRegister));
        assert_eq!(ecam_address(usize::MAX, location, 0x1000),
                   Err(PciProbeError::InvalidRegister));
        assert_eq!(ecam_address(usize::MAX, PciLocation { bus : 1, device : 0, function : 0 }, 0),
                   Err(PciProbeError::AddressOverflow));
    }

    #[test]
    fn probe_is_read_only_and_decodes_identity() {
        let result = probe(&Fixture { vendor_device : 0x1000_0014,
                                      class_register : 0x0200_0000 },
                           PciLocation { bus : 0, device : 3, function : 0 });
        assert_eq!(result,
                   PciProbeResult::Present(PciIdentity {
                       location : PciLocation { bus : 0, device : 3, function : 0 },
                       vendor_id : 0x0014, device_id : 0x1000,
                       class_code : 0x02, subclass : 0, prog_if : 0,
                   }));
    }

    #[test]
    fn all_ones_vendor_is_absent() {
        assert_eq!(probe(&Fixture { vendor_device : 0xffff_ffff, class_register : 0 },
                         PciLocation { bus : 0, device : 3, function : 0 }),
                   PciProbeResult::Absent);
    }

    #[test]
    fn volatile_reader_rejects_unmapped_or_too_small_windows() {
        assert!(matches!(unsafe {
                            VolatileConfigReader::from_region(api_v0::MmioRegion { base : 0,
                                                                                    size : 4096 })
                        },
                        Err(PciProbeError::InvalidWindow)));
        assert!(matches!(unsafe {
                            VolatileConfigReader::from_region(api_v0::MmioRegion { base : 0x1002,
                                                                                    size : 4096 })
                        },
                        Err(PciProbeError::InvalidWindow)));
        assert!(matches!(unsafe {
                            VolatileConfigReader::from_region(api_v0::MmioRegion { base : 0x1000_0000,
                                                                                    size : 3 })
                        },
                        Err(PciProbeError::InvalidWindow)));
    }
}
