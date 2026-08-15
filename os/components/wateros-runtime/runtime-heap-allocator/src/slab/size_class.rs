//! slab size class：固定对象大小表与 layout 路由。

use core::alloc::Layout;

/// slab 单页大小（与 WaterOS frame allocator 页大小一致）。
pub(crate) const SLAB_PAGE_SIZE : usize = 4096;

/// 页首 header 预留字节数，保证对象起始地址 64 字节对齐。
pub(crate) const SLAB_HEADER_SIZE : usize = 64;

/// 走 slab 的最大对象大小；超过则交给大对象路径。
pub(crate) const SLAB_MAX_SIZE : usize = 2048;

pub(crate) const SIZE_CLASS_COUNT : usize = 16;

/// 固定 size class 表：覆盖常见小对象，避免纯 2 的幂导致过多浪费。
pub(crate) const SIZE_CLASS_SIZES : [usize; SIZE_CLASS_COUNT] = [
    8, 16, 24, 32, 48, 64, 96, 128,
    192, 256, 384, 512, 768, 1024, 1536, 2048,
];

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
        let need = layout.size()
                          .max(layout.align())
                          .max(1);
        let idx = SIZE_CLASS_SIZES.iter()
                                  .position(|size| *size >= need)?;
        Some(Self(idx))
    }

    pub(crate) fn index(self) -> usize { self.0 }

    pub(crate) fn size(self) -> usize { SIZE_CLASS_SIZES[self.0] }

    /// 一页 slab 能容纳的对象数（扣除页首 header）。
    pub(crate) fn objects_per_slab(self) -> usize {
        let offset = align_up(SLAB_HEADER_SIZE, self.size());
        (SLAB_PAGE_SIZE - offset) / self.size()
    }
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
}
