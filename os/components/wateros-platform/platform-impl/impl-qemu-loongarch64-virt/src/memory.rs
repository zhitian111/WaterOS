//! 平台物理内存布局：从引导 DTB 解析 RAM 上界（bring-up 期供 MM 初始化）。

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
