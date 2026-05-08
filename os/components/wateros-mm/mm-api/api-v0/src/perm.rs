//! 页权限位集合（语义层）；与 Sv39 PTE 的 R/W/X/U 对应关系由 `mm-impl` 翻译，**不**包含 A/D/G 等硬件管理位。

/// 页权限（语义层，不包含页表 PTE 具体 bit 编码）。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagePerm(pub u8);

impl PagePerm {
    /// 可读
    pub const R: Self = Self(1 << 0);
    /// 可写
    pub const W: Self = Self(1 << 1);
    /// 可执行
    pub const X: Self = Self(1 << 2);
    /// 用户态可访问
    pub const U: Self = Self(1 << 3);

    /// 空权限（非法映射的常见前置；实现可拒绝或按需扩展）。
    #[inline]
    pub const fn empty() -> Self { Self(0) }

    /// 是否含可读位。
    #[inline]
    pub const fn readable(self) -> bool { (self.0 & Self::R.0) != 0 }
    /// 是否含可写位。
    #[inline]
    pub const fn writable(self) -> bool { (self.0 & Self::W.0) != 0 }
    /// 是否含可执行位。
    #[inline]
    pub const fn executable(self) -> bool { (self.0 & Self::X.0) != 0 }
    /// 是否含用户可访问位。
    #[inline]
    pub const fn user(self) -> bool { (self.0 & Self::U.0) != 0 }

    /// 原始权限位（实现层映射到 PTE 时使用）。
    #[inline]
    pub const fn bits(self) -> u8 { self.0 }

    /// 与 `other` 按位或后的新权限集合。
    #[inline]
    pub const fn with(self, other: Self) -> Self { Self(self.0 | other.0) }
}

impl core::ops::BitOr for PagePerm {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output { Self(self.0 | rhs.0) }
}

impl core::ops::BitOrAssign for PagePerm {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) { self.0 |= rhs.0; }
}

/// `PagePerm` 查询与组合的单元测试。
pub fn test() {
    log::trace!("[mm-api::perm] test begin");
    let p = PagePerm::R | PagePerm::W | PagePerm::U;
    assert!(p.readable());
    assert!(p.writable());
    assert!(!p.executable());
    assert!(p.user());
    log::trace!("[mm-api::perm] test end");
}

