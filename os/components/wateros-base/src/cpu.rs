//! 无平台策略的逻辑 CPU 标识和 CPU 集合类型。

use core::cell::UnsafeCell;
use config::task::MAX_CPUS;

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
    /// WaterOS 当前配置的全部逻辑 CPU。
    pub const ALL : Self = Self(if MAX_CPUS >= u64::BITS as usize {
        u64::MAX
    } else {
        (1u64 << MAX_CPUS) - 1
    });
    pub const fn from_bits(bits : u64) -> Self { Self(bits) }

    /// 从 Linux cpu_set_t 使用的小端字节序解析掩码。
    ///
    /// `CpuMask` 只能表达 64 个 CPU；若第 8 个字节之后仍有置位 bit，返回
    /// `None`，而不是静默丢弃调用者的 affinity 请求。
    pub fn try_from_le_bytes(bytes : &[u8]) -> Option<Self> {
        if bytes.get(core::mem::size_of::<u64>()..)
                .is_some_and(|tail| tail.iter()
                                         .any(|byte| *byte != 0))
        {
            return None;
        }
        let mut bits = 0;
        for (i, byte) in bytes.iter()
                            .enumerate()
        {
            if i >= core::mem::size_of::<u64>() {
                break;
            }
            bits |= (*byte as u64) << (i * 8);
        }
        Some(Self(bits))
    }

    /// 将掩码以 Linux cpu_set_t 使用的小端字节序写入调用者缓冲区。
    /// 超出本掩码宽度的字节必须是 0，避免向 userspace 泄漏旧缓冲区内容。
    pub fn write_le_bytes(self, out : &mut [u8]) {
        out.fill(0);
        let bytes = self.bits()
                        .to_le_bytes();
        let len = out.len()
                     .min(bytes.len());
        out[..len].copy_from_slice(&bytes[..len]);
    }
    pub fn to_vec(&self, out : &mut [u8]) {
        for (i, byte) in out.iter_mut()
                            .enumerate()
        {
            if i >= 8 {
                break;
            }
            *byte = ((self.0 >> (i * 8)) & 0xFF) as u8;
        }
    }
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
pub struct CpuLocal<T, const N: usize> {
    slots : [UnsafeCell<T>; N],
}

unsafe impl<T : Sync, const N: usize> Sync for CpuLocal<T, N> {}

impl<T, const N: usize> CpuLocal<T, N> {
    pub fn new(values : [T; N]) -> Self { Self { slots : values.map(UnsafeCell::new) } }

    /// 用已经包装好的槽构造静态 CPU-local 存储。
    pub const fn from_cells(slots : [UnsafeCell<T>; N]) -> Self { Self { slots } }

    pub const fn capacity(&self) -> usize { N }

    pub fn get(&self, cpu : CpuId) -> Option<&T> {
        self.slots
            .get(cpu.index())
            .map(|slot| unsafe { &*slot.get() })
    }

    /// 获取当前 CPU 独占拥有的槽位。
    ///
    /// # Safety
    /// 调用方必须保证该槽位没有任何并发读写，通常要求 `cpu` 就是当前 CPU，且
    /// 其它 CPU 永远不修改这个槽位。
    pub unsafe fn get_local_mut(&self, cpu : CpuId) -> Option<&mut T> {
        self.slots
            .get(cpu.index())
            .map(|slot| unsafe { &mut *slot.get() })
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
    fn cpu_mask_round_trips_linux_little_endian_bytes() {
        let mask = CpuMask::from_bits((1 << 0) | (1 << 9) | (1 << 63));
        let mut bytes = [0xff; 12];
        mask.write_le_bytes(&mut bytes);
        assert_eq!(bytes[0], 0b0000_0001);
        assert_eq!(bytes[1], 0b0000_0010);
        assert_eq!(bytes[7], 0b1000_0000);
        assert!(bytes[8..].iter().all(|byte| *byte == 0));
        assert_eq!(CpuMask::try_from_le_bytes(&bytes), Some(mask));
    }

    #[test]
    fn cpu_mask_rejects_bits_outside_its_64_bit_capacity() {
        assert_eq!(CpuMask::try_from_le_bytes(&[0; 9]), Some(CpuMask::EMPTY));
        assert_eq!(CpuMask::try_from_le_bytes(&[0, 0, 0, 0, 0, 0, 0, 0, 1]), None);
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
        assert!(local.get(CpuId::from_raw(2))
                     .is_none());
    }
}
