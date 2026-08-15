//! slab size class：固定对象大小表与 layout 路由。

use core::alloc::Layout;

/// slab 单页大小（与 WaterOS frame allocator 页大小一致）。
pub(crate) const SLAB_PAGE_SIZE : usize = 4096;

/// 页首 header 实际 `repr(C)` 大小（含对齐 padding）。
pub(crate) const SLAB_HEADER_SIZE : usize = 72;

/// 走 slab 的最大对象大小；超过则交给大对象路径。
pub(crate) const SLAB_MAX_SIZE : usize = 2048;

pub(crate) const SIZE_CLASS_COUNT : usize = 16;

/// 固定 size class 表：覆盖常见小对象，避免纯 2 的幂导致过多浪费。
pub(crate) const SIZE_CLASS_SIZES : [usize; SIZE_CLASS_COUNT] = [
    8, 16, 24, 32, 48, 64, 96, 128,
    192, 256, 384, 512, 768, 1024, 1536, 2048,
];

const CLASS_INDEX_BY_NEED : [u8; SLAB_MAX_SIZE + 1] = {
    let mut table = [u8::MAX; SLAB_MAX_SIZE + 1];
    let mut class = 0usize;
    let mut need = 1usize;
    while need <= SLAB_MAX_SIZE {
        while class < SIZE_CLASS_COUNT && SIZE_CLASS_SIZES[class] < need {
            class += 1;
        }
        if class < SIZE_CLASS_COUNT {
            table[need] = class as u8;
        }
        need += 1;
    }
    table
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SizeClass(usize);

impl SizeClass {
    /// 按索引构造；索引必须来自 [`SIZE_CLASS_SIZES`] 的合法下标。
    pub(crate) fn from_index(idx : usize) -> Self { Self(idx) }

    /// 从 `Layout` 选择最小可用 size class；对齐或大小超出 slab 范围返回 `None`。
    pub(crate) fn from_layout(layout : Layout) -> Option<Self> {
        if layout.size() > SLAB_MAX_SIZE || layout.align() > SLAB_MAX_SIZE {
            return None;
        }
        let need = align_up(layout.size().max(1), layout.align());
        let idx = CLASS_INDEX_BY_NEED[need];
        if idx == u8::MAX {
            None
        } else {
            Some(Self(idx as usize))
        }
    }

    pub(crate) fn index(self) -> usize { self.0 }

    pub(crate) fn size(self) -> usize { SIZE_CLASS_SIZES[self.0] }

    /// 返回该 size class 能保证的最大 2 次幂对象对齐。
    ///
    /// 对象以 `size` 为步长排布，因此只有 size 的最大 2 次幂因子可以同时
    /// 保证页内每个对象的地址对齐。此前对非 2 次幂 class 使用“小于 size
    /// 的最大 2 次幂”，会高估 `24/48/96/...` 类的对齐能力并产生未对齐对象。
    pub(crate) fn object_align(self) -> usize {
        object_align_for_size(self.size())
    }

    /// 返回第一个对象相对页基址的偏移，保证满足该类最大对齐要求。
    pub(crate) fn object_offset(self) -> usize {
        align_up(SLAB_HEADER_SIZE, self.object_align())
    }

    /// 一页 slab 能容纳的对象数（扣除页首 header）。
    pub(crate) fn objects_per_slab(self) -> usize {
        let offset = self.object_offset();
        (SLAB_PAGE_SIZE - offset) / self.size()
    }
}

/// `size` 中最低置位的 1，等于能同时整除 `size` 的最大 2 次幂。
fn object_align_for_size(size : usize) -> usize {
    size & size.wrapping_neg()
}

fn align_up(value : usize, align : usize) -> usize {
    (value + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_smallest_fitting_class() {
        let layout = Layout::from_size_align(17, 8).unwrap();
        let class = SizeClass::from_layout(layout).unwrap();
        assert_eq!(class.size(), 24);
    }

    #[test]
    fn alignment_raises_class() {
        let layout = Layout::from_size_align(16, 64).unwrap();
        let class = SizeClass::from_layout(layout).unwrap();
        assert_eq!(class.size(), 64);
    }

    #[test]
    fn non_power_of_two_alignment_preserves_object_stride() {
        let class = SizeClass::from_layout(Layout::from_size_align(17, 16).unwrap())
                        .unwrap();
        assert_eq!(class.size(), 32);
        assert_eq!(class.object_align(), 32);

        let class24 = SizeClass::from_layout(Layout::from_size_align(17, 8).unwrap())
                          .unwrap();
        assert_eq!(class24.size(), 24);
        assert_eq!(class24.object_align(), 8);
    }

    #[test]
    fn object_align_is_largest_power_of_two_divisor() {
        for (size, expected) in [(24, 8), (48, 16), (96, 32), (192, 64)] {
            assert_eq!(object_align_for_size(size), expected);
        }
    }
}
