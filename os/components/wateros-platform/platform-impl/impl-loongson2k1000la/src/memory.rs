use api_v0::memory::{KernelMemoryLayout, PhysicalRange};

const MMIO : [PhysicalRange; 2] = [PhysicalRange::new(0x1000_0000, 0x3000_0000),
                                   PhysicalRange::new(0x4000_0000, 0x8000_0000)];

/// Conservative 1 GiB-board high-memory window. Boot-param parsing must replace
/// this fallback before boards with a different fitted RAM size are supported.
pub const fn physical_ram_end_exclusive() -> usize { 0xC000_0000 }

pub fn kernel_memory_layout() -> KernelMemoryLayout {
    KernelMemoryLayout { ram : PhysicalRange::new(0x9000_0000,
                                                  physical_ram_end_exclusive()),
                         mmio : &MMIO,
                         probe_virtual_page : None }.validate()
                                                    .expect("2K1000LA fallback memory layout must \
                                                             be valid")
}

#[cfg(test)]
mod tests {
    #[test]
    fn fallback_layout_is_valid() {
        assert!(super::kernel_memory_layout().validate()
                                             .is_ok());
    }
}
