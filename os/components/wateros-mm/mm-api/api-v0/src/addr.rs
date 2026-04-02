/// MM 语义层地址/页粒度类型（与具体页表实现无关）。
///
/// 这些类型只做“数值语义封装 + 对齐/分解计算”，不包含任何硬件页表编码细节。

pub const PAGE_SIZE: usize = 4096;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(pub usize);

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(pub usize);

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtPageNum(pub usize);

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysPageNum(pub usize);

impl VirtAddr {
    #[inline]
    pub const fn page_offset(self) -> usize { self.0 & (PAGE_SIZE - 1) }

    #[inline]
    pub const fn floor_page(self) -> VirtPageNum { VirtPageNum(self.0 / PAGE_SIZE) }

    #[inline]
    pub const fn ceil_page(self) -> VirtPageNum {
        VirtPageNum((self.0 + PAGE_SIZE - 1) / PAGE_SIZE)
    }

    #[inline]
    pub const fn page_start(self) -> VirtAddr {
        VirtAddr((self.floor_page().0) * PAGE_SIZE)
    }
}

impl PhysAddr {
    #[inline]
    pub const fn page_offset(self) -> usize { self.0 & (PAGE_SIZE - 1) }

    #[inline]
    pub const fn floor_page(self) -> PhysPageNum { PhysPageNum(self.0 / PAGE_SIZE) }

    #[inline]
    pub const fn ceil_page(self) -> PhysPageNum {
        PhysPageNum((self.0 + PAGE_SIZE - 1) / PAGE_SIZE)
    }

    #[inline]
    pub const fn page_start(self) -> PhysAddr {
        PhysAddr((self.floor_page().0) * PAGE_SIZE)
    }
}

impl VirtPageNum {
    #[inline]
    pub const fn start_addr(self) -> VirtAddr { VirtAddr(self.0 * PAGE_SIZE) }
}

impl PhysPageNum {
    #[inline]
    pub const fn start_addr(self) -> PhysAddr { PhysAddr(self.0 * PAGE_SIZE) }
}

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

