//! 平台提供的物理内存布局契约。
//!
//! 描述内核恒等映射的主 RAM 连续区间与 MMIO 区间，以及（可选的）用于验证新安装页表的
//! 探测虚拟页。物理板的实现应从 DTB/固件推导布局；QEMU profile 提供固定布局。
//!
//! 契约刻意只描述内核的主连续 RAM 区间；不连续的底层内存池可在不修改架构 MM 实现的
//! 前提下，以后按板扩展。

/// 页大小（RISC-V Sv39 与 LoongArch64 均为 4 KiB）。
const PAGE_SIZE : usize = 4096;

/// 物理地址闭开区间 `[start, end)`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalRange {
    pub start : usize,
    pub end : usize,
}

impl PhysicalRange {
    pub const fn new(start : usize, end : usize) -> Self {
        Self { start, end }
    }

    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }

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

/// 内存布局校验失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryLayoutError {
    /// RAM 区间为空。
    EmptyRam,
    /// RAM 区间未按页对齐。
    UnalignedRam,
    /// 某个 MMIO 区间为空。
    EmptyMmio,
    /// 某个 MMIO 区间未按页对齐。
    UnalignedMmio,
    /// MMIO 与 RAM 重叠。
    RamMmioOverlap,
    /// MMIO 区间之间相互重叠。
    MmioOverlap,
    /// 探测虚拟页未按页对齐。
    UnalignedProbeVirtualPage,
    /// 探测虚拟页与 RAM/MMIO 重叠。
    ProbeVirtualPageOverlap,
}

/// 架构 MM 实现将恒等映射的物理区域。
///
/// `probe_virtual_page` 是可选的、其它用途未占用的虚拟页，用于验证新安装的页表；
/// RISC-V 需要它，LoongArch 通过 RAM 恒等映射探测并使用 `None`。
#[derive(Clone, Copy, Debug)]
pub struct KernelMemoryLayout {
    pub ram : PhysicalRange,
    pub mmio : &'static [PhysicalRange],
    pub probe_virtual_page : Option<usize>,
}

impl KernelMemoryLayout {
    /// 校验布局一致性：RAM/MMIO 非空、页对齐且互不重叠。
    pub fn validate(self) -> Result<Self, MemoryLayoutError> {
        if self.ram.is_empty() {
            return Err(MemoryLayoutError::EmptyRam);
        }
        if !self.ram.is_page_aligned() {
            return Err(MemoryLayoutError::UnalignedRam);
        }
        for (index, range) in self.mmio.iter().copied().enumerate() {
            if range.is_empty() {
                return Err(MemoryLayoutError::EmptyMmio);
            }
            if !range.is_page_aligned() {
                return Err(MemoryLayoutError::UnalignedMmio);
            }
            if self.ram.overlaps(range) {
                return Err(MemoryLayoutError::RamMmioOverlap);
            }
            if self.mmio[..index]
                .iter()
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
            if self.ram.overlaps(probe_range)
                || self
                    .mmio
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

    const MMIO : [PhysicalRange; 2] = [
        PhysicalRange::new(0x1000, 0x2000),
        PhysicalRange::new(0x3000, 0x4000),
    ];

    #[test]
    fn accepts_disjoint_page_aligned_regions() {
        let layout = KernelMemoryLayout {
            ram : PhysicalRange::new(0x8000, 0x20_000),
            mmio : &MMIO,
            probe_virtual_page : Some(0x40_000),
        };
        assert!(layout.validate().is_ok());
    }

    #[test]
    fn rejects_ram_mmio_and_probe_overlap() {
        const BAD_MMIO : [PhysicalRange; 1] = [PhysicalRange::new(0x9000, 0xA000)];
        let overlap = KernelMemoryLayout {
            ram : PhysicalRange::new(0x8000, 0x20_000),
            mmio : &BAD_MMIO,
            probe_virtual_page : None,
        };
        assert_eq!(
            overlap.validate().unwrap_err(),
            MemoryLayoutError::RamMmioOverlap
        );

        let probe = KernelMemoryLayout {
            ram : PhysicalRange::new(0x8000, 0x20_000),
            mmio : &[],
            probe_virtual_page : Some(0x10_000),
        };
        assert_eq!(
            probe.validate().unwrap_err(),
            MemoryLayoutError::ProbeVirtualPageOverlap
        );
    }

    #[test]
    fn rejects_overlapping_mmio_regions() {
        const OVERLAP : [PhysicalRange; 2] = [
            PhysicalRange::new(0x1000, 0x3000),
            PhysicalRange::new(0x2000, 0x4000),
        ];
        let layout = KernelMemoryLayout {
            ram : PhysicalRange::new(0x8000, 0x20_000),
            mmio : &OVERLAP,
            probe_virtual_page : None,
        };
        assert_eq!(
            layout.validate().unwrap_err(),
            MemoryLayoutError::MmioOverlap
        );
    }
}
