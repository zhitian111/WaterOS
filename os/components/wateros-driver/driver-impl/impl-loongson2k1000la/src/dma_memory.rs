//! Owned physically-contiguous memory for 2K1000LA DMA engines.
//!
//! `UNVERIFIED_ON_HARDWARE`: current kernel profiles identity-map allocatable
//! RAM, but the exact 2K1000LA cache and DMA visibility rules still require a
//! board. This module deliberately requires a caller-supplied coherency backend;
//! it does not treat identity mapping as proof of cache coherence.

#[cfg(target_arch = "loongarch64")]
use api_v0::{
    dma::{DmaCoherency, DmaDirection, DmaMapping, DmaRegion},
    DriverResult,
};
#[cfg(target_arch = "loongarch64")]
use frame_allocator::{FrameAllocError, OwnedPhysFrameSpan};
#[cfg(any(test, target_arch = "loongarch64"))]
use mm_api::addr::PAGE_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaAllocationError {
    InvalidParam,
    OutOfMemory,
    Unsupported,
}

#[cfg(target_arch = "loongarch64")]
impl From<FrameAllocError> for DmaAllocationError {
    fn from(error : FrameAllocError) -> Self {
        match error {
            FrameAllocError::InvalidFrame => Self::InvalidParam,
            FrameAllocError::OutOfMemory => Self::OutOfMemory,
            FrameAllocError::Unsupported => Self::Unsupported,
        }
    }
}

#[cfg(any(test, target_arch = "loongarch64"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AllocationLayout {
    frame_count : usize,
    alignment_frames : usize,
}

#[cfg(any(test, target_arch = "loongarch64"))]
fn allocation_layout(byte_length : usize,
                     byte_alignment : usize)
                     -> Result<AllocationLayout, DmaAllocationError> {
    if byte_length == 0 || byte_alignment == 0 || !byte_alignment.is_power_of_two() {
        return Err(DmaAllocationError::InvalidParam);
    }
    let frame_count = byte_length.checked_add(PAGE_SIZE - 1)
                                 .ok_or(DmaAllocationError::InvalidParam)? /
                      PAGE_SIZE;
    let alignment_frames = if byte_alignment <= PAGE_SIZE {
        1
    } else {
        if byte_alignment % PAGE_SIZE != 0 {
            return Err(DmaAllocationError::InvalidParam);
        }
        byte_alignment / PAGE_SIZE
    };
    Ok(AllocationLayout { frame_count,
                          alignment_frames })
}

/// DMA mapping tied to the lifetime of its physically-contiguous allocation.
#[cfg(target_arch = "loongarch64")]
pub struct OwnedDmaBuffer<C> {
    memory : Option<OwnedPhysFrameSpan>,
    mapping : DmaMapping<C>,
    byte_length : usize,
}

#[cfg(target_arch = "loongarch64")]
impl<C : DmaCoherency> OwnedDmaBuffer<C> {
    /// Allocate identity-mapped RAM and create a mapping for the exact requested
    /// byte prefix. Page tail bytes remain owned but are never exposed to DMA.
    pub fn allocate_zeroed(byte_length : usize,
                           byte_alignment : usize,
                           device_address_bits : u8,
                           direction : DmaDirection,
                           coherency : C)
                           -> Result<Self, DmaAllocationError> {
        // Current bring-up uses the kernel's identity mapping explicitly;
        // callers that have a separate mapping must use `allocate_zeroed_at`.
        Self::allocate_zeroed_at(byte_length,
                                 byte_alignment,
                                 device_address_bits,
                                 direction,
                                 coherency,
                                 None)
    }

    /// Allocate a DMA buffer while explicitly supplying the virtual mapping
    /// used by the device driver. `None` retains the current identity-map
    /// bring-up wrapper; `Some` never changes the owned physical allocation.
    pub fn allocate_zeroed_at(byte_length : usize,
                              byte_alignment : usize,
                              device_address_bits : u8,
                              direction : DmaDirection,
                              coherency : C,
                              virtual_address : Option<usize>)
                              -> Result<Self, DmaAllocationError> {
        let layout = allocation_layout(byte_length, byte_alignment)?;
        let memory = OwnedPhysFrameSpan::alloc_zeroed(layout.frame_count,
                                                      layout.alignment_frames)?;
        let physical_address = memory.physical_address()
                                     .0;
        let virtual_address = virtual_address.unwrap_or(physical_address);
        let region =
            DmaRegion::new(virtual_address,
                           physical_address as u64,
                           byte_length,
                           byte_alignment,
                           device_address_bits).map_err(|_| DmaAllocationError::InvalidParam)?;
        Ok(Self { memory : Some(memory),
                  mapping : DmaMapping::new(region, direction, coherency),
                  byte_length })
    }

    pub fn cpu_bytes(&self) -> DriverResult<&[u8]> {
        self.mapping
            .cpu_region()?;
        Ok(&self.memory
                .as_ref()
                .expect("owned DMA memory missing")
                .as_bytes()[..self.byte_length])
    }

    pub fn cpu_bytes_mut(&mut self) -> DriverResult<&mut [u8]> {
        self.mapping
            .cpu_region()?;
        Ok(&mut self.memory
                    .as_mut()
                    .expect("owned DMA memory missing")
                    .as_bytes_mut()[..self.byte_length])
    }

    pub fn region(&self) -> DriverResult<DmaRegion> {
        self.mapping
            .cpu_region()
    }

    pub(crate) fn mapping_mut(&mut self) -> &mut DmaMapping<C> { &mut self.mapping }
}

#[cfg(target_arch = "loongarch64")]
impl<C> Drop for OwnedDmaBuffer<C> {
    fn drop(&mut self) {
        if !self.mapping
                .is_cpu_owned()
        {
            if let Some(memory) = self.memory.take() {
                log::error!("[driver-ls2k][dma] retaining {} device-owned bytes; hardware was \
                             not quiesced (UNVERIFIED_ON_HARDWARE)",
                            self.byte_length);
                core::mem::forget(memory);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_v0::{dma::DmaRegion, DriverError};

    #[test]
    fn rounds_byte_length_without_weakening_alignment() {
        assert_eq!(allocation_layout(1, 32),
                   Ok(AllocationLayout { frame_count : 1,
                                         alignment_frames : 1 }));
        assert_eq!(allocation_layout(PAGE_SIZE + 1, PAGE_SIZE * 4),
                   Ok(AllocationLayout { frame_count : 2,
                                         alignment_frames : 4 }));
    }

    #[test]
    fn rejects_invalid_or_overflowing_layouts() {
        assert_eq!(allocation_layout(0, 32),
                   Err(DmaAllocationError::InvalidParam));
        assert_eq!(allocation_layout(64, 3),
                   Err(DmaAllocationError::InvalidParam));
        assert_eq!(allocation_layout(usize::MAX, PAGE_SIZE),
                   Err(DmaAllocationError::InvalidParam));
    }

    #[test]
    fn identity_region_still_enforces_device_address_width() {
        assert_eq!(DmaRegion::new(0x1_0000_0000,
                                  0x1_0000_0000,
                                  4096,
                                  4096,
                                  32),
                   Err(DriverError::InvalidParam));
    }
}
