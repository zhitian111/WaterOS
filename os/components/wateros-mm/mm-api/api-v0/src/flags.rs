/// mmap 的语义标志（先做最小子集）。
///
/// 说明：这里不强制完全等同 Linux 的全部 `MAP_*` 数值，只提供语义子集与可扩展的位集合。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapFlags(pub u32);

impl MapFlags {
    /// 匿名映射（无文件后备）
    pub const ANONYMOUS: Self = Self(1 << 0);
    /// 私有映射（fork/写时复制语义由后续实现）
    pub const PRIVATE: Self = Self(1 << 1);
    /// 共享映射（当前阶段可先按 PRIVATE 语义拒绝或延后实现）
    pub const SHARED: Self = Self(1 << 2);

    #[inline]
    pub const fn empty() -> Self { Self(0) }

    #[inline]
    pub const fn bits(self) -> u32 { self.0 }

    #[inline]
    pub const fn contains(self, other: Self) -> bool { (self.0 & other.0) == other.0 }
}

impl core::ops::BitOr for MapFlags {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output { Self(self.0 | rhs.0) }
}

impl core::ops::BitOrAssign for MapFlags {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) { self.0 |= rhs.0; }
}

pub fn test() {
    log::trace!("[mm-api::flags] test begin");
    let f = MapFlags::ANONYMOUS | MapFlags::PRIVATE;
    assert!(f.contains(MapFlags::ANONYMOUS));
    assert!(f.contains(MapFlags::PRIVATE));
    assert!(!f.contains(MapFlags::SHARED));
    log::trace!("[mm-api::flags] test end");
}

