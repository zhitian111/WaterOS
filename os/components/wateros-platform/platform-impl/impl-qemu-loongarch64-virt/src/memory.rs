//! 平台物理内存布局：以 [`KernelMemoryLayout`] 契约描述 RAM 与恒等映射 MMIO；
//! RAM 上界从引导 DTB 解析（含 QEMU 8.x 编码修正），失败时用回退值。

use api_v0::memory::{KernelMemoryLayout, PhysicalRange};

/// QEMU LoongArch64 `virt` 的 RAM 基址：内核从该物理地址启动。
const QEMU_RAM_BASE : usize = 0x9000_0000;

/// QEMU LoongArch64 `virt` 恒等映射 MMIO 区间。
const MMIO_RANGES : [PhysicalRange; 2] = [
    PhysicalRange::new(0x1000_0000, 0x3000_0000),
    PhysicalRange::new(0x4000_0000, 0x8000_0000),
];

/// 当前平台的 RAM/MMIO 布局；LoongArch 通过 RAM 恒等映射探测，不使用探测虚拟页。
pub fn kernel_memory_layout() -> KernelMemoryLayout {
    KernelMemoryLayout {
        ram : PhysicalRange::new(QEMU_RAM_BASE, detect_ram_end_exclusive()),
        mmio : &MMIO_RANGES,
        probe_virtual_page : None,
    }
    .validate()
    .expect("QEMU LoongArch64 memory layout must be valid")
}

/// 物理 RAM 上界（不包含）：由 [`kernel_memory_layout`] 派生，保持单一事实来源。
pub fn physical_ram_end_exclusive() -> usize {
    kernel_memory_layout().ram.end
}

/// 修正部分 QEMU 8.x LoongArch `virt` 直接内核启动生成的异常 `reg` 编码。
///
/// 该版本把 `qemu_fdt_setprop_sized_cells(..., 2, value, ...)` 中描述宽度的
/// `2` 也留在了 64 位值的高 32 位，于是 `0x9000_0000/0x3000_0000` 被通用
/// FDT 解析器读成 `0x2_9000_0000/0x2_3000_0000`。这些地址不属于 guest
/// RAM，直接交给帧分配器会在第一次清零页表或 DMA 页时触发 page fault。
/// 新版 QEMU 生成的标准 64 位 cell 高半部为 0，不会进入此兼容分支。
#[inline]
fn normalize_qemu8_region(base : usize, size : usize) -> (usize, usize) {
    if (base >> 32) == 2 && (size >> 32) == 2 {
        (base & u32::MAX as usize, size & u32::MAX as usize)
    } else {
        (base, size)
    }
}

/// 物理 RAM 上界（不包含）：QEMU LoongArch64 `virt -m 1G` 把前 256 MiB
/// 放在 `0..0x1000_0000`，其余 768 MiB 放在
/// `0x9000_0000..0xc000_0000`。内核从 `0x9000_0000` 启动，帧分配器只能
/// 使用包含内核的高 RAM 段，不能跨过中间 MMIO/空洞。
fn detect_ram_end_exclusive() -> usize {
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
                    let raw_base = region.starting_address as usize;
                    let Some(raw_size) = region.size else {
                        continue;
                    };
                    let (base, size) = normalize_qemu8_region(raw_base, raw_size);
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
    0xc000_0000
}

#[cfg(test)]
mod tests {
    use super::normalize_qemu8_region;

    #[test]
    fn accepts_standard_64_bit_region() {
        assert_eq!(normalize_qemu8_region(0x9000_0000, 0x3000_0000),
                   (0x9000_0000, 0x3000_0000));
    }

    #[test]
    fn repairs_qemu8_loongarch_sized_cells() {
        assert_eq!(normalize_qemu8_region(0x2_9000_0000, 0x2_3000_0000),
                   (0x9000_0000, 0x3000_0000));
    }
}
