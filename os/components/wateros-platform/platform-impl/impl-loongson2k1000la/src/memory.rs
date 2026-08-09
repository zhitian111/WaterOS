//! Loongson 2K1000LA physical memory discovery.

use api_v0::memory::{KernelMemoryLayout, PhysicalRange};

const KERNEL_LINK_ADDRESS : usize = 0x9000_0000;
const FALLBACK_RAM_END : usize = 0xC000_0000;
const PAGE_MASK : usize = 4096 - 1;

const MMIO : [PhysicalRange; 2] = [PhysicalRange::new(0x1000_0000, 0x3000_0000),
                                   PhysicalRange::new(0x4000_0000, 0x8000_0000)];

fn page_aligned_candidate(base : usize, size : usize) -> Option<PhysicalRange> {
    let end = base.checked_add(size)?;
    let start = base.checked_add(PAGE_MASK)? & !PAGE_MASK;
    let end = end & !PAGE_MASK;
    let range = PhysicalRange::new(start, end);
    (!range.is_empty() && range.contains(KERNEL_LINK_ADDRESS)).then_some(range)
}

fn prefer_larger(current : Option<PhysicalRange>, candidate : PhysicalRange) -> PhysicalRange {
    match current {
        Some(current) if current.end - current.start >= candidate.end - candidate.start => current,
        _ => candidate,
    }
}

/// Selects the page-aligned RAM extent containing the linked kernel image.
///
/// The current platform/MM API represents one contiguous primary RAM extent.
/// The official 2K1000LA DTS contains discontiguous low-memory regions, so they
/// are intentionally left unused instead of being merged across address holes.
pub fn primary_ram_from_fdt(fdt : &fdt::Fdt<'_>) -> Option<PhysicalRange> {
    let mut selected = None;
    for node in fdt.all_nodes() {
        let is_memory = node.name == "memory" ||
                        node.name
                            .starts_with("memory@");
        if !is_memory {
            continue;
        }
        let Some(regions) = node.reg() else {
            continue;
        };
        for region in regions {
            let Some(size) = region.size else {
                continue;
            };
            let base = region.starting_address as usize;
            if let Some(candidate) = page_aligned_candidate(base, size) {
                selected = Some(prefer_larger(selected, candidate));
            }
        }
    }
    selected
}

fn discovered_primary_ram() -> Option<PhysicalRange> {
    let dtb_pa = crate::dtb::dtb_pa();
    if dtb_pa == 0 {
        return None;
    }
    // SAFETY: `dtb_pa` is captured from the validated early firmware boot path
    // and consumed before its identity mapping is replaced. Real-firmware
    // lifetime and reserved-memory interactions remain to be verified on board.
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) }.ok()?;
    primary_ram_from_fdt(&fdt)
}

pub fn primary_ram() -> PhysicalRange {
    discovered_primary_ram().unwrap_or(PhysicalRange::new(KERNEL_LINK_ADDRESS, FALLBACK_RAM_END))
}

pub fn physical_ram_end_exclusive() -> usize { primary_ram().end }

pub fn kernel_memory_layout() -> KernelMemoryLayout {
    KernelMemoryLayout { ram : primary_ram(),
                         mmio : &MMIO,
                         probe_virtual_page : None }.validate()
                                                    .expect("2K1000LA primary memory layout must \
                                                             be valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_layout_is_valid() {
        assert_eq!(primary_ram(),
                   PhysicalRange::new(KERNEL_LINK_ADDRESS, FALLBACK_RAM_END));
        assert!(kernel_memory_layout().validate()
                                      .is_ok());
    }

    #[test]
    fn selects_and_page_aligns_kernel_extent() {
        assert_eq!(page_aligned_candidate(0x8FFF_F123, 0x2000),
                   Some(PhysicalRange::new(0x9000_0000, 0x9000_1000)));
        assert_eq!(page_aligned_candidate(0x0020_0000, 0x06E0_0000),
                   None);
    }

    #[test]
    fn rejects_empty_and_overflowing_extents() {
        assert_eq!(page_aligned_candidate(KERNEL_LINK_ADDRESS, 0),
                   None);
        assert_eq!(page_aligned_candidate(usize::MAX - 0x1000, 0x2000),
                   None);
    }

    #[test]
    fn keeps_larger_overlapping_candidate() {
        let small = PhysicalRange::new(KERNEL_LINK_ADDRESS, 0xA000_0000);
        let large = PhysicalRange::new(0x8000_0000, 0x1_0000_0000);
        assert_eq!(prefer_larger(Some(small), large), large);
        assert_eq!(prefer_larger(Some(large), small), large);
    }
}
