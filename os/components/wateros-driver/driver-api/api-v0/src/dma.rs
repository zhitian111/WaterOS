//! DMA address and cache-ownership contract for physical devices.
//!
//! This API never assumes virtual and physical addresses are identical. It
//! models ownership transitions only; allocation, address translation and the
//! architecture-specific cache implementation remain platform responsibilities.

use crate::{DriverError, DriverResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    ToDevice,
    FromDevice,
    Bidirectional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmaRegion {
    virtual_address : usize,
    physical_address : u64,
    length : usize,
    alignment : usize,
}

impl DmaRegion {
    pub fn new(virtual_address : usize,
               physical_address : u64,
               length : usize,
               alignment : usize,
               device_address_bits : u8)
               -> DriverResult<Self> {
        if virtual_address == 0 || length == 0 || alignment == 0 || !alignment.is_power_of_two() ||
           virtual_address % alignment != 0 || physical_address % alignment as u64 != 0 ||
           !(1..=64).contains(&device_address_bits)
        {
            return Err(DriverError::InvalidParam);
        }
        virtual_address.checked_add(length - 1).ok_or(DriverError::InvalidParam)?;
        let end = physical_address.checked_add(length as u64 - 1)
                                  .ok_or(DriverError::InvalidParam)?;
        if device_address_bits < 64 && end >= 1u64 << device_address_bits {
            return Err(DriverError::InvalidParam);
        }
        Ok(Self { virtual_address, physical_address, length, alignment })
    }

    pub const fn virtual_address(self) -> usize { self.virtual_address }
    pub const fn physical_address(self) -> u64 { self.physical_address }
    pub const fn length(self) -> usize { self.length }
    pub const fn alignment(self) -> usize { self.alignment }

    /// Physical end address (exclusive), retaining overflow checks.
    pub fn physical_end_exclusive(self) -> DriverResult<u64> {
        self.physical_address
            .checked_add(self.length as u64)
            .ok_or(DriverError::InvalidParam)
    }

    /// Derive a bounded subrange without allowing callers to escape the
    /// original physically contiguous mapping.
    pub fn subregion(self,
                     offset : usize,
                     length : usize,
                     alignment : usize,
                     device_address_bits : u8)
                     -> DriverResult<Self> {
        let end = offset.checked_add(length).ok_or(DriverError::InvalidParam)?;
        if length == 0 || end > self.length {
            return Err(DriverError::InvalidParam);
        }
        let virtual_address = self.virtual_address
                                  .checked_add(offset)
                                  .ok_or(DriverError::InvalidParam)?;
        let physical_address = self.physical_address
                                   .checked_add(offset as u64)
                                   .ok_or(DriverError::InvalidParam)?;
        Self::new(virtual_address,
                  physical_address,
                  length,
                  alignment,
                  device_address_bits)
    }
}

/// Platform cache and ordering operations for one physically contiguous region.
pub trait DmaCoherency {
    /// Make CPU writes visible before the device starts and publish descriptors.
    fn sync_for_device(&mut self,
                       region : DmaRegion,
                       direction : DmaDirection)
                       -> DriverResult<()>;
    /// Make device writes visible before the CPU reads a completed buffer.
    fn sync_for_cpu(&mut self,
                    region : DmaRegion,
                    direction : DmaDirection)
                    -> DriverResult<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    Cpu,
    Device,
}

/// A contiguous DMA mapping with enforced CPU/device ownership transitions.
pub struct DmaMapping<C> {
    region : DmaRegion,
    direction : DmaDirection,
    owner : Owner,
    coherency : C,
}

impl<C : DmaCoherency> DmaMapping<C> {
    pub fn new(region : DmaRegion, direction : DmaDirection, coherency : C) -> Self {
        Self { region, direction, owner : Owner::Cpu, coherency }
    }

    pub fn cpu_region(&self) -> DriverResult<DmaRegion> {
        (self.owner == Owner::Cpu).then_some(self.region).ok_or(DriverError::InvalidParam)
    }

    pub fn prepare_for_device(&mut self) -> DriverResult<DmaRegion> {
        if self.owner != Owner::Cpu { return Err(DriverError::InvalidParam); }
        self.coherency.sync_for_device(self.region, self.direction)?;
        self.owner = Owner::Device;
        Ok(self.region)
    }

    pub fn complete_from_device(&mut self) -> DriverResult<DmaRegion> {
        if self.owner != Owner::Device { return Err(DriverError::InvalidParam); }
        self.coherency.sync_for_cpu(self.region, self.direction)?;
        self.owner = Owner::Cpu;
        Ok(self.region)
    }

    /// Recover ownership after a confirmed hardware stop. The backend still
    /// performs CPU-side synchronization because a partial device write may exist.
    pub fn reclaim_after_stop(&mut self) -> DriverResult<DmaRegion> {
        self.complete_from_device()
    }

    pub fn into_parts(self) -> DriverResult<(DmaRegion, C)> {
        if self.owner != Owner::Cpu { return Err(DriverError::InvalidParam); }
        Ok((self.region, self.coherency))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Event { Device(DmaDirection), Cpu(DmaDirection) }
    #[derive(Default)]
    struct MockCoherency { events : Vec<Event>, fail_device : bool }
    impl DmaCoherency for MockCoherency {
        fn sync_for_device(&mut self, _region : DmaRegion, direction : DmaDirection)
                           -> DriverResult<()> {
            if self.fail_device { return Err(DriverError::IoError); }
            self.events.push(Event::Device(direction));
            Ok(())
        }
        fn sync_for_cpu(&mut self, _region : DmaRegion, direction : DmaDirection)
                        -> DriverResult<()> {
            self.events.push(Event::Cpu(direction));
            Ok(())
        }
    }

    #[test]
    fn validates_distinct_addresses_alignment_and_device_width() {
        let region = DmaRegion::new(0x4000, 0x1_0000_8000, 4096, 4096, 64).unwrap();
        assert_eq!(region.virtual_address(), 0x4000);
        assert_eq!(region.physical_address(), 0x1_0000_8000);
        assert_eq!(DmaRegion::new(0x4001, 0x8000, 4, 4, 32),
                   Err(DriverError::InvalidParam));
        assert_eq!(DmaRegion::new(0x4000, 0xffff_f000, 8192, 4096, 32),
                   Err(DriverError::InvalidParam));
    }

    #[test]
    fn enforces_sync_and_exclusive_ownership_sequence() {
        let region = DmaRegion::new(0x4000, 0x8000, 512, 32, 32).unwrap();
        let mut mapping = DmaMapping::new(region, DmaDirection::FromDevice,
                                          MockCoherency::default());
        assert_eq!(mapping.complete_from_device(), Err(DriverError::InvalidParam));
        assert_eq!(mapping.prepare_for_device(), Ok(region));
        assert_eq!(mapping.cpu_region(), Err(DriverError::InvalidParam));
        assert_eq!(mapping.prepare_for_device(), Err(DriverError::InvalidParam));
        assert_eq!(mapping.complete_from_device(), Ok(region));
        let (_, backend) = mapping.into_parts().unwrap();
        assert_eq!(backend.events,
                   [Event::Device(DmaDirection::FromDevice),
                    Event::Cpu(DmaDirection::FromDevice)]);
    }

    #[test]
    fn failed_sync_does_not_transfer_ownership() {
        let region = DmaRegion::new(0x4000, 0x8000, 64, 32, 32).unwrap();
        let mut mapping = DmaMapping::new(region, DmaDirection::ToDevice,
                                          MockCoherency { fail_device : true,
                                                          ..MockCoherency::default() });
        assert_eq!(mapping.prepare_for_device(), Err(DriverError::IoError));
        assert_eq!(mapping.cpu_region(), Ok(region));
    }

    #[test]
    fn bounded_subregions_preserve_distinct_addresses_and_reject_escape() {
        let region = DmaRegion::new(0x8000, 0x1_0000_8000, 4096, 64, 64).unwrap();
        assert_eq!(region.physical_end_exclusive(), Ok(0x1_0000_9000));
        let child = region.subregion(512, 1024, 64, 64).unwrap();
        assert_eq!(child.virtual_address(), 0x8200);
        assert_eq!(child.physical_address(), 0x1_0000_8200);
        assert_eq!(child.length(), 1024);
        assert_eq!(region.subregion(4096, 1, 1, 64), Err(DriverError::InvalidParam));
        assert_eq!(region.subregion(0, 0, 64, 64), Err(DriverError::InvalidParam));
        assert_eq!(region.subregion(1, 64, 64, 64), Err(DriverError::InvalidParam));
    }
}
