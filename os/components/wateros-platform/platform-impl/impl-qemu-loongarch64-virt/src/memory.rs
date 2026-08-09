//! QEMU LoongArch64 physical RAM and identity-mapped MMIO layout.

use api_v0::memory::{KernelMemoryLayout, PhysicalRange};

const QEMU_RAM_BASE : usize = 0x9000_0000;
const MMIO_RANGES : [PhysicalRange; 2] =
    [PhysicalRange::new(0x1000_0000, 0x3000_0000),
     PhysicalRange::new(0x4000_0000, 0x8000_0000)];

/// 物理 RAM 上界（不包含）：QEMU LoongArch64 `virt -m 1G` 的可用 RAM
/// 包含内核所在的 `0x8000_0000..0xb000_0000` 高段；内核从 `0x9000_0000`
/// 启动，因此 frame allocator 的高段 fallback 必须停在 `0xb000_0000`，
/// 不能把中间 MMIO/空洞当作 RAM。
pub fn physical_ram_end_exclusive() -> usize {
    let dtb_pa = crate::dtb::dtb_pa();
    if dtb_pa != 0 {
        if let Ok(fdt) = unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) } {
            let mut best_end = 0usize;
            for node in fdt.all_nodes() {
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
                    if end > base && base >= 0x8000_0000 && end > best_end {
                        best_end = end;
                    }
                }
            }
            if best_end > 0x8000_0000 {
                return best_end;
            }
        }
    }
    // 回退：DTB 不可用时匹配仓库 Makefile 的 QEMU `virt -m 1G`。
    0xb000_0000
}

pub fn kernel_memory_layout() -> KernelMemoryLayout {
    KernelMemoryLayout { ram : PhysicalRange::new(QEMU_RAM_BASE,
                                                   physical_ram_end_exclusive()),
                         mmio : &MMIO_RANGES,
                         probe_virtual_page : None }
        .validate()
        .expect("QEMU LoongArch64 memory layout must be valid")
}
