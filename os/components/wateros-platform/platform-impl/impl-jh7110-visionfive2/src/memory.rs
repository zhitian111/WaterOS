//! 物理内存布局占位：任务 05 改为从 DTB 推导；当前用保守回退值保证契约可校验。

use api_v0::memory::{KernelMemoryLayout, PhysicalRange};

pub fn kernel_memory_layout() -> KernelMemoryLayout {
    const NO_MMIO : [PhysicalRange; 0] = [];
    KernelMemoryLayout {
        ram : PhysicalRange::new(config::mm::QEMU_VIRT_PHYS_RAM_BASE,
                                 config::mm::QEMU_VIRT_PHYS_RAM_END),
        mmio : &NO_MMIO,
        probe_virtual_page : Some(0x4000_0000),
    }
    .validate()
    .expect("stub JH7110 memory layout must be valid")
}

pub fn physical_ram_end_exclusive() -> usize {
    kernel_memory_layout().ram.end
}
