//! 物理/虚拟地址及页号的新类型包装，用于在通用代码中区分不同地址空间含义。

#[allow(unused)]
/// 物理地址（未附加偏移或对齐保证；调用方负责与 MMU/IO 映射约定一致）。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BasePhysAddr {
    /// 裸物理地址数值。
    pub val : usize,
}
#[allow(unused)]
/// 虚拟地址（未附加地址空间标识；是否用户/内核由使用场景决定）。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaseVirtAddr {
    /// 裸虚拟地址数值。
    pub val : usize,
}

#[allow(unused)]
/// 物理页号（PPN），不包含页内偏移。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BasePPN {
    /// 页号数值（具体编码与 Sv39 等页表格式对齐由上层 MM 实现解释）。
    pub val : usize,
}
#[allow(unused)]
/// 虚拟页号（VPN），不包含页内偏移。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaseVPN {
    /// 页号数值（与具体分页级数相关的拆分由上层 MM 实现负责）。
    pub val : usize,
}

impl<T> Into<*mut T> for BasePhysAddr {
    /// 将物理地址数值解释为可写内核指针；调用方需保证该映射在内核地址空间有效。
    #[inline]
    #[allow(unused)]
    fn into(self) -> *mut T {
        self.val as *mut T
    }
}
impl<T> Into<*mut T> for BaseVirtAddr {
    /// 将虚拟地址数值解释为可写指针；调用方需保证当前地址空间下该地址可访问。
    #[inline]
    #[allow(unused)]
    fn into(self) -> *mut T {
        self.val as *mut T
    }
}
