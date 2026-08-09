//! Board-provided physical memory and identity-mapped MMIO layout.
//!
//! The contract deliberately describes the kernel's primary contiguous RAM
//! range. Discontiguous low-memory pools can be added later without teaching
//! an architecture MM implementation about individual boards.

const PAGE_SIZE : usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalRange {
    pub start : usize,
    pub end : usize,
}

impl PhysicalRange {
    pub const fn new(start : usize, end : usize) -> Self { Self { start, end } }

    pub const fn is_empty(self) -> bool { self.start >= self.end }

    pub const fn contains(self, address : usize) -> bool {
        address >= self.start && address < self.end
    }

    pub const fn overlaps(self, other : Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    pub const fn is_page_aligned(self) -> bool {
        self.start % PAGE_SIZE == 0 && self.end % PAGE_SIZE == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryLayoutError {
    EmptyRam,
    UnalignedRam,
    EmptyMmio,
    UnalignedMmio,
    RamMmioOverlap,
    MmioOverlap,
    UnalignedProbeVirtualPage,
    ProbeVirtualPageOverlap,
}

/// Physical regions that the architecture MM implementation identity maps.
///
/// `probe_virtual_page` is an optional otherwise-unused virtual page used to
/// verify a newly installed page table. RISC-V currently needs it; LoongArch
/// probes through the RAM identity map and uses `None`.
#[derive(Clone, Copy, Debug)]
pub struct KernelMemoryLayout {
    pub ram : PhysicalRange,
    pub mmio : &'static [PhysicalRange],
    pub probe_virtual_page : Option<usize>,
}

impl KernelMemoryLayout {
    pub fn validate(self) -> Result<Self, MemoryLayoutError> {
        if self.ram.is_empty() {
            return Err(MemoryLayoutError::EmptyRam);
        }
        if !self.ram
                .is_page_aligned()
        {
            return Err(MemoryLayoutError::UnalignedRam);
        }
        for (index, range) in self.mmio
                                  .iter()
                                  .copied()
                                  .enumerate()
        {
            if range.is_empty() {
                return Err(MemoryLayoutError::EmptyMmio);
            }
            if !range.is_page_aligned() {
                return Err(MemoryLayoutError::UnalignedMmio);
            }
            if self.ram
                   .overlaps(range)
            {
                return Err(MemoryLayoutError::RamMmioOverlap);
            }
            if self.mmio[..index].iter()
                                 .copied()
                                 .any(|prior| prior.overlaps(range))
            {
                return Err(MemoryLayoutError::MmioOverlap);
            }
        }
        if let Some(probe) = self.probe_virtual_page {
            if probe % PAGE_SIZE != 0 {
                return Err(MemoryLayoutError::UnalignedProbeVirtualPage);
            }
            let probe_range = PhysicalRange::new(probe, probe.saturating_add(PAGE_SIZE));
            if self.ram
                   .overlaps(probe_range) ||
               self.mmio
                   .iter()
                   .copied()
                   .any(|range| range.overlaps(probe_range))
            {
                return Err(MemoryLayoutError::ProbeVirtualPageOverlap);
            }
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{KernelMemoryLayout, MemoryLayoutError, PhysicalRange};

    const MMIO : [PhysicalRange; 2] = [PhysicalRange::new(0x1000, 0x2000),
                                       PhysicalRange::new(0x3000, 0x4000)];

    #[test]
    fn accepts_disjoint_page_aligned_regions() {
        let layout = KernelMemoryLayout { ram : PhysicalRange::new(0x8000, 0x20_000),
                                          mmio : &MMIO,
                                          probe_virtual_page : Some(0x40_000) };
        assert!(layout.validate()
                      .is_ok());
    }

    #[test]
    fn rejects_ram_mmio_and_probe_overlap() {
        const BAD_MMIO : [PhysicalRange; 1] = [PhysicalRange::new(0x9000, 0xA000)];
        let overlap = KernelMemoryLayout { ram : PhysicalRange::new(0x8000, 0x20_000),
                                           mmio : &BAD_MMIO,
                                           probe_virtual_page : None };
        assert_eq!(overlap.validate()
                          .unwrap_err(),
                   MemoryLayoutError::RamMmioOverlap);

        let probe = KernelMemoryLayout { ram : PhysicalRange::new(0x8000, 0x20_000),
                                         mmio : &[],
                                         probe_virtual_page : Some(0x10_000) };
        assert_eq!(probe.validate()
                        .unwrap_err(),
                   MemoryLayoutError::ProbeVirtualPageOverlap);
    }

    #[test]
    fn rejects_overlapping_mmio_regions() {
        const OVERLAP : [PhysicalRange; 2] = [PhysicalRange::new(0x1000, 0x3000),
                                              PhysicalRange::new(0x2000, 0x4000)];
        let layout = KernelMemoryLayout { ram : PhysicalRange::new(0x8000, 0x20_000),
                                          mmio : &OVERLAP,
                                          probe_virtual_page : None };
        assert_eq!(layout.validate()
                         .unwrap_err(),
                   MemoryLayoutError::MmioOverlap);
    }
}
