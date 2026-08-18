//! mmap 相关标志的语义子集；数值与 Linux `MAP_*` 不必一致，由 syscall 层与本类型共同约定。

/// mmap 的语义标志（先做最小子集）。
///
/// 说明：这里不强制完全等同 Linux 的全部 `MAP_*` 数值，只提供语义子集与可扩展的位集合。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapFlags(
    /// 内部语义位集合；syscall 层负责把 Linux `MAP_*` 数值转换为这些位并拒绝未知组合。
    pub u32,
);

impl MapFlags {
    /// 匿名映射（无文件后备）；初始内容由实现清零或按缺页策略提供。
    pub const ANONYMOUS: Self = Self(1 << 0);
    /// 私有映射；写入不应传播回文件，fork/写时复制的具体机制由实现决定。
    pub const PRIVATE: Self = Self(1 << 1);
    /// 共享映射；当前阶段可明确拒绝或延后实现，不得悄悄按 PRIVATE 成功处理。
    pub const SHARED: Self = Self(1 << 2);
    /// 固定地址映射（若目标已映射则失败；语义子集，与 Linux `MAP_FIXED` 对齐程度见 syscall 层）。
    pub const FIXED: Self = Self(1 << 4);
    /// 固定地址但禁止替换任何既有映射；用于 `shmat` 未指定 `SHM_REMAP` 的语义。
    pub const FIXED_NOREPLACE: Self = Self(1 << 5);

    /// 无任何标志位。
    #[inline]
    pub const fn empty() -> Self { Self(0) }

    /// 原始位模式；仅供 ABI 适配和调试使用，调用者不能据此绕过未知位校验。
    #[inline]
    pub const fn bits(self) -> u32 { self.0 }

    /// 是否包含 `other` 的全部置位（子集检测）。
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

/// `MapFlags` 位运算与 `contains` 的单元测试。
pub fn test() {
    log::trace!("[mm-api::flags] test begin");
    let f = MapFlags::ANONYMOUS | MapFlags::PRIVATE;
    assert!(f.contains(MapFlags::ANONYMOUS));
    assert!(f.contains(MapFlags::PRIVATE));
    assert!(!f.contains(MapFlags::SHARED));
    log::trace!("[mm-api::flags] test end");
}
