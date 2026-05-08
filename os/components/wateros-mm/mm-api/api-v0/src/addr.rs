//! MM 语义层地址与页号类型（与具体页表编码无关）。
//!
//! ## 固定假设
//!
//! - [`PAGE_SIZE`] 为 **4096**（4 KiB），与当前 WaterOS RISC-V Sv39 叶子页一致；若将来支持非 4K 页，需整体调整本模块与 `mm-impl`。
//! - `VirtAddr`/`PhysAddr` 为 **字节地址**；`VirtPageNum`/`PhysPageNum` 为 **页号**（非 PPN 硬件字段移位后的值，而是 `addr / PAGE_SIZE`）。

/// 页大小（字节）；与 RISC-V Sv39 4KiB 叶子页及本仓库 `mm-impl` 一致。
pub const PAGE_SIZE: usize = 4096;

/// 用户或内核虚拟字节地址。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(pub usize);

/// 物理字节地址（语义层；是否可解引用取决于平台是否恒等映射等）。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(pub usize);

/// 虚拟页号：`VirtAddr::floor_page` / `ceil_page` 与 [`PAGE_SIZE`] 对齐分解得到。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtPageNum(pub usize);

/// 物理页号：与 [`PhysAddr`] 的关系为 `ppn * PAGE_SIZE`（与 Sv39 PTE 中 PPN 字段同一粒度时可数值对齐）。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysPageNum(pub usize);

impl VirtAddr {
    /// 页内字节偏移 `[0, PAGE_SIZE)`。
    #[inline]
    pub const fn page_offset(self) -> usize { self.0 & (PAGE_SIZE - 1) }

    /// 向下对齐到页边界对应的虚拟页号。
    #[inline]
    pub const fn floor_page(self) -> VirtPageNum { VirtPageNum(self.0 / PAGE_SIZE) }

    /// 向上对齐到页边界对应的虚拟页号（`va` 恰在页边界时与 `floor_page` 相同）。
    #[inline]
    pub const fn ceil_page(self) -> VirtPageNum {
        VirtPageNum((self.0 + PAGE_SIZE - 1) / PAGE_SIZE)
    }

    /// 当前虚拟地址所在页的起始字节地址。
    #[inline]
    pub const fn page_start(self) -> VirtAddr {
        VirtAddr((self.floor_page().0) * PAGE_SIZE)
    }
}

impl PhysAddr {
    /// 页内字节偏移 `[0, PAGE_SIZE)`。
    #[inline]
    pub const fn page_offset(self) -> usize { self.0 & (PAGE_SIZE - 1) }

    /// 向下对齐到物理页号。
    #[inline]
    pub const fn floor_page(self) -> PhysPageNum { PhysPageNum(self.0 / PAGE_SIZE) }

    /// 向上对齐到物理页号。
    #[inline]
    pub const fn ceil_page(self) -> PhysPageNum {
        PhysPageNum((self.0 + PAGE_SIZE - 1) / PAGE_SIZE)
    }

    /// 当前物理地址所在页的起始字节地址。
    #[inline]
    pub const fn page_start(self) -> PhysAddr {
        PhysAddr((self.floor_page().0) * PAGE_SIZE)
    }
}

impl VirtPageNum {
    /// 该虚拟页的起始字节地址。
    #[inline]
    pub const fn start_addr(self) -> VirtAddr { VirtAddr(self.0 * PAGE_SIZE) }

    /// 恒等映射下的物理页号（`vpn == ppn`），仅用于早期 bring-up / 内核直映射区间。
    #[inline]
    pub const fn to_phys_page_identity(self) -> PhysPageNum { PhysPageNum(self.0) }
}

impl PhysPageNum {
    /// 该物理页的起始字节地址。
    #[inline]
    pub const fn start_addr(self) -> PhysAddr { PhysAddr(self.0 * PAGE_SIZE) }
}

/// 地址类型与页对齐运算的单元测试（日志级 trace，无硬件依赖）。
pub fn test() {
    log::trace!("[mm-api::addr] test begin");

    let va = VirtAddr(0x1234);
    assert_eq!(va.page_offset(), 0x234);
    assert_eq!(va.floor_page(), VirtPageNum(0x1));
    assert_eq!(va.ceil_page(), VirtPageNum(0x2));
    assert_eq!(va.page_start(), VirtAddr(0x1000));

    let pa = PhysAddr(0x8fff);
    assert_eq!(pa.page_offset(), 0xfff);
    assert_eq!(pa.floor_page(), PhysPageNum(0x8));
    assert_eq!(pa.ceil_page(), PhysPageNum(0x9));
    assert_eq!(pa.page_start(), PhysAddr(0x8000));

    log::trace!("[mm-api::addr] test end");
}

