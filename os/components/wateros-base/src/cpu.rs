//! 无平台策略的逻辑 CPU 标识和 CPU 集合类型。

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

#[cfg(test)]
mod tests {
    use super::{CpuId, CpuMask};

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
}
