//! 无平台策略的逻辑 CPU 标识和 CPU 集合类型。

use core::cell::UnsafeCell;

/// 逻辑 CPU 标识。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CpuId(usize);

impl CpuId {
    /// 引导 CPU 的默认逻辑编号。
    pub const BOOT : Self = Self(0);

    pub const fn from_raw(raw : usize) -> Self { Self(raw) }

    pub const fn raw(self) -> usize { self.0 }

    pub const fn index(self) -> usize { self.0 }

    pub const fn fits_capacity(self, capacity : usize) -> bool { self.0 < capacity }
}

/// 一组逻辑 CPU。容量校验由使用方依据平台配置完成。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuMask(u64);

impl CpuMask {
    pub const EMPTY : Self = Self(0);

    pub const fn from_bits(bits : u64) -> Self { Self(bits) }

    pub const fn bits(self) -> u64 { self.0 }

    pub const fn contains(self, cpu : CpuId) -> bool {
        cpu.raw() < u64::BITS as usize && self.0 & (1u64 << cpu.raw()) != 0
    }

    pub fn insert(&mut self, cpu : CpuId) {
        assert!(cpu.raw() < u64::BITS as usize,
                "CpuId does not fit CpuMask");
        self.0 |= 1u64 << cpu.raw();
    }

    pub fn remove(&mut self, cpu : CpuId) {
        assert!(cpu.raw() < u64::BITS as usize,
                "CpuId does not fit CpuMask");
        self.0 &= !(1u64 << cpu.raw());
    }

    pub const fn count(self) -> usize { self.0.count_ones() as usize }

    pub const fn is_empty(self) -> bool { self.0 == 0 }
}

/// 旧接口兼容别名。新代码应使用 [`CpuId`]。
pub type CPUHartID = usize;

/// 固定容量、无需堆分配的 CPU-local 存储。
///
/// 本类型只负责按 [`CpuId`] 做边界检查。跨 CPU 共享槽位时，`T` 自身仍须提供
/// 所需同步；典型用法是存放原子变量或每个 CPU 只修改自己槽位的状态。
pub struct CpuLocal<T, const N : usize> {
    slots : [UnsafeCell<T>; N],
}

unsafe impl<T : Sync, const N : usize> Sync for CpuLocal<T, N> {}

impl<T, const N : usize> CpuLocal<T, N> {
    pub fn new(values : [T; N]) -> Self {
        Self { slots: values.map(UnsafeCell::new) }
    }

    /// 用已经包装好的槽构造静态 CPU-local 存储。
    pub const fn from_cells(slots : [UnsafeCell<T>; N]) -> Self { Self { slots } }

    pub const fn capacity(&self) -> usize { N }

    pub fn get(&self, cpu : CpuId) -> Option<&T> {
        self.slots.get(cpu.index()).map(|slot| unsafe { &*slot.get() })
    }

    /// 获取当前 CPU 独占拥有的槽位。
    ///
    /// # Safety
    /// 调用方必须保证该槽位没有任何并发读写，通常要求 `cpu` 就是当前 CPU，且
    /// 其它 CPU 永远不修改这个槽位。
    pub unsafe fn get_local_mut(&self, cpu : CpuId) -> Option<&mut T> {
        self.slots.get(cpu.index()).map(|slot| unsafe { &mut *slot.get() })
    }
}

#[cfg(test)]
mod tests {
    use super::{CpuId, CpuLocal, CpuMask};

    #[test]
    fn cpu_mask_tracks_membership() {
        let mut mask = CpuMask::EMPTY;
        mask.insert(CpuId::BOOT);
        mask.insert(CpuId::from_raw(7));
        assert!(mask.contains(CpuId::BOOT));
        assert!(mask.contains(CpuId::from_raw(7)));
        assert_eq!(mask.count(), 2);
        mask.remove(CpuId::BOOT);
        assert_eq!(mask.bits(), 1 << 7);
    }

    #[test]
    fn out_of_range_cpu_is_not_contained() {
        assert!(!CpuMask::from_bits(u64::MAX).contains(CpuId::from_raw(64)));
    }

    #[test]
    fn cpu_local_checks_capacity() {
        let local = CpuLocal::new([10, 20]);
        assert_eq!(local.get(CpuId::BOOT), Some(&10));
        assert_eq!(local.get(CpuId::from_raw(1)), Some(&20));
        assert!(local.get(CpuId::from_raw(2)).is_none());
    }
}
