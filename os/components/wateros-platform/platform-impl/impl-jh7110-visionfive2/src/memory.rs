//! VisionFive 2 / JH7110 物理内存布局：RAM 上界从引导 DTB 推导，失败时用回退值。

use api_v0::memory::{KernelMemoryLayout, PhysicalRange};

const RAM_BASE : usize = 0x4000_0000;
const FALLBACK_RAM_END : usize = 0x8000_0000;
const MMIO : [PhysicalRange; 1] = [PhysicalRange::new(0x0100_0000, RAM_BASE)];

pub fn physical_ram_end_exclusive() -> usize {
    let dtb_pa = crate::dtb::dtb_pa();
    if dtb_pa != 0 {
        if let Ok(fdt) = unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) } {
            let mut best = 0usize;
            for node in fdt.all_nodes()
                           .filter(|node| {
                               node.name
                                   .starts_with("memory")
                           })
            {
                if let Some(regions) = node.reg() {
                    for region in regions {
                        if let Some(size) = region.size {
                            let start = region.starting_address as usize;
                            let end = start.saturating_add(size);
                            if start <= RAM_BASE && end > best {
                                best = end;
                            }
                        }
                    }
                }
            }
            if best > RAM_BASE {
                return best & !0xFFF;
            }
        }
    }
    FALLBACK_RAM_END
}

pub fn kernel_memory_layout() -> KernelMemoryLayout {
    KernelMemoryLayout {
        ram : PhysicalRange::new(RAM_BASE, physical_ram_end_exclusive()),
        mmio : &MMIO,
        probe_virtual_page : Some(0x0020_0000),
    }
    .validate()
    .expect("VisionFive 2 memory layout must be valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_layout_is_valid() {
        crate::dtb::store(0);
        assert!(kernel_memory_layout().validate().is_ok());
    }

    #[test]
    fn ram_end_falls_back_without_dtb() {
        crate::dtb::store(0);
        assert_eq!(physical_ram_end_exclusive(), FALLBACK_RAM_END);
    }
}
