//! QEMU RISC-V physical RAM and identity-mapped MMIO layout.

use api_v0::memory::{KernelMemoryLayout, PhysicalRange};

const MMIO_RANGES : [PhysicalRange; 2] =
    [PhysicalRange::new(config::mm::QEMU_VIRT_MMIO_PHYS_START,
                        config::mm::QEMU_VIRT_MMIO_PHYS_END),
     PhysicalRange::new(config::mm::QEMU_VIRT_RTC_PHYS_START,
                        config::mm::QEMU_VIRT_RTC_PHYS_END)];

/// 物理 RAM 上界（不包含）：优先解析平台 DTB `memory@*` 的 `reg`；失败时用
/// `wateros-base-config` 回退值。
pub fn physical_ram_end_exclusive() -> usize {
    use config::mm::QEMU_VIRT_PHYS_RAM_END as FALLBACK;
    let dtb_pa = crate::dtb::dtb_pa();
    if dtb_pa == 0 {
        return FALLBACK;
    }
    let Ok(fdt) = (unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) }) else {
        return FALLBACK;
    };
    let mut best_end = 0usize;
    for node in fdt.all_nodes() {
        // 与 Linux/QEMU virt 常见命名一致；非规范强制，属当前 bring-up 假设。
        if !node.name.starts_with("memory") {
            continue;
        }
        let Some(mut regions) = node.reg() else {
            continue;
        };
        while let Some(region) = regions.next() {
            let base = region.starting_address as usize;
            let Some(size) = region.size else {
                continue;
            };
            let end = base.saturating_add(size);
            // 忽略低于 DRAM 典型起点的区域，避免误选保留映射。
            if end > base && base >= 0x8000_0000 && end > best_end {
                best_end = end;
            }
        }
    }
    if best_end > 0x8000_0000 {
        best_end
    } else {
        FALLBACK
    }
}

pub fn kernel_memory_layout() -> KernelMemoryLayout {
    KernelMemoryLayout { ram : PhysicalRange::new(config::mm::QEMU_VIRT_PHYS_RAM_BASE,
                                                   physical_ram_end_exclusive()),
                         mmio : &MMIO_RANGES,
                         probe_virtual_page : Some(0x4000_0000) }
        .validate()
        .expect("QEMU RISC-V memory layout must be valid")
}
