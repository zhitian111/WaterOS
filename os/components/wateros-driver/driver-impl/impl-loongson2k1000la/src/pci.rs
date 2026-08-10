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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciConfigSnapshot {
    pub identity : PciIdentity,
    pub bars : [Option<PciBar>; 6],
    pub bar_error : Option<PciBarError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciSnapshotResult {
    Absent,
    Present(PciConfigSnapshot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciBar {
    Io { index : u8, base : u32 },
    Memory32 { index : u8, base : u32, prefetchable : bool },
    Memory64 { index : u8, base : u64, prefetchable : bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciBarError {
    InvalidIndex,
    MissingUpperHalf,
    UnsupportedMemoryType,
    Unassigned,
}

/// Decode one already-read PCI BAR without probing or writing configuration.
pub fn parse_bar(index : u8, low : u32, high : Option<u32>) -> Result<PciBar, PciBarError> {
    if index >= 6 {
        return Err(PciBarError::InvalidIndex);
    }
    if low == 0 || low == u32::MAX {
        return Err(PciBarError::Unassigned);
    }
    if low & 1 != 0 {
        return Ok(PciBar::Io { index,
                               base : low & !3 });
    }
    let memory_type = (low >> 1) & 3;
    let prefetchable = low & 8 != 0;
    match memory_type {
        0 => Ok(PciBar::Memory32 { index,
                                   base : low & !0xf,
                                   prefetchable }),
        2 => Ok(PciBar::Memory64 { index,
                                   base : (u64::from(high.ok_or(PciBarError::MissingUpperHalf)? ) << 32) |
                                          u64::from(low & !0xf),
                                   prefetchable }),
        _ => Err(PciBarError::UnsupportedMemoryType),
    }
}

/// Read identity and BARs without writing PCI configuration space.
pub fn probe_snapshot<R : ConfigReader>(reader : &R,
                                        location : PciLocation)
                                        -> PciSnapshotResult {
    let identity = match probe(reader, location) {
        PciProbeResult::Absent => return PciSnapshotResult::Absent,
        PciProbeResult::Present(identity) => identity,
    };
    let mut bars = [None; 6];
    let mut bar_error = None;
    let mut index = 0u8;
    while index < 6 {
        let low = reader.read32(ecam_offset(location, 0x10 + u16::from(index) * 4).unwrap());
        let memory64 = low & 1 == 0 && ((low >> 1) & 3) == 2;
        let high = if memory64 && index < 5 {
            Some(reader.read32(ecam_offset(location,
                                           0x10 + u16::from(index + 1) * 4)
                                         .unwrap()))
        } else {
            None
        };
        match parse_bar(index, low, high) {
            Ok(bar) => {
                bars[usize::from(index)] = Some(bar);
                if memory64 {
                    index += 1;
                }
            }
            Err(PciBarError::Unassigned) => {
                if memory64 && index == 5 {
                    bar_error = Some(PciBarError::MissingUpperHalf);
                } else if memory64 {
                    index += 1;
                }
            }
            Err(error) => {
                bar_error.get_or_insert(error);
            }
        }
        index += 1;
    }
    PciSnapshotResult::Present(PciConfigSnapshot { identity, bars, bar_error })
}

pub const fn bar_is_assigned(bar : Result<PciBar, PciBarError>) -> bool {
    matches!(bar, Ok(PciBar::Io { base, .. }) if base != 0) ||
    matches!(bar, Ok(PciBar::Memory32 { base, .. }) if base != 0) ||
    matches!(bar, Ok(PciBar::Memory64 { base, .. }) if base != 0)
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

/// Perform one explicit, read-only function snapshot from a mapped ECAM
/// region. This helper is intentionally not called by `init_after_boot`:
/// DTB ECAM ownership and mapping attributes still require board evidence.
pub unsafe fn probe_volatile_snapshot(region : api_v0::MmioRegion,
                                       location : PciLocation)
                                       -> Result<PciSnapshotResult, PciProbeError> {
    let reader = unsafe { VolatileConfigReader::from_region(region)? };
    Ok(probe_snapshot(&reader, location))
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

    struct SnapshotFixture;
    impl ConfigReader for SnapshotFixture {
        fn read32(&self, offset : u64) -> u32 {
            match offset & 0xfff {
                0x00 => 0x1000_0014,
                0x08 => 0x0200_0000,
                0x10 => 0x8000_0008,
                0x14 => 0,
                0x18 => 0x0000_0004,
                0x1c => 0x0000_0001,
                _ => 0,
            }
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

    #[test]
    fn bar_decoder_handles_io_32_and_64_bit_memory_bars() {
        assert_eq!(parse_bar(0, 0x0000_1001, None),
                   Ok(PciBar::Io { index : 0, base : 0x1000 }));
        assert_eq!(parse_bar(1, 0x8000_0008, None),
                   Ok(PciBar::Memory32 { index : 1,
                                         base : 0x8000_0000,
                                         prefetchable : true }));
        assert_eq!(parse_bar(2, 0x0000_0004, Some(0x0000_0001)),
                   Ok(PciBar::Memory64 { index : 2,
                                         base : 0x1_0000_0000,
                                         prefetchable : false }));
        assert!(bar_is_assigned(parse_bar(0, 0x1001, None)));
    }

    #[test]
    fn bar_decoder_rejects_unassigned_and_malformed_bars() {
        assert_eq!(parse_bar(0, 0, None), Err(PciBarError::Unassigned));
        assert_eq!(parse_bar(6, 0x1000, None), Err(PciBarError::InvalidIndex));
        assert_eq!(parse_bar(0, 0x0000_0004, None), Err(PciBarError::MissingUpperHalf));
        assert_eq!(parse_bar(0, 0x0000_0006, None), Err(PciBarError::UnsupportedMemoryType));
        assert!(!bar_is_assigned(parse_bar(0, 0, None)));
    }

    #[test]
    fn snapshot_reads_identity_and_consumes_64_bit_bar_pair() {
        let PciSnapshotResult::Present(snapshot) =
            probe_snapshot(&SnapshotFixture,
                           PciLocation { bus : 0, device : 3, function : 0 })
        else { panic!("fixture unexpectedly absent") };
        assert_eq!(snapshot.identity.vendor_id, 0x0014);
        assert_eq!(snapshot.bars[0],
                   Some(PciBar::Memory32 { index : 0,
                                           base : 0x8000_0000,
                                           prefetchable : true }));
        assert_eq!(snapshot.bars[1], None);
        assert_eq!(snapshot.bars[2],
                   Some(PciBar::Memory64 { index : 2,
                                           base : 0x1_0000_0000,
                                           prefetchable : false }));
        assert_eq!(snapshot.bar_error, None);
    }
}
