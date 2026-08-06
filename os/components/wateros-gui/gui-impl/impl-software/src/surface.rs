//! Shadow framebuffer 与有界脏矩形集合。

use alloc::{vec, vec::Vec};
use api_v0::{GuiError, GuiResult, Rect, Size};

pub const BYTES_PER_PIXEL : usize = 4;
pub const MAX_DIRTY_REGIONS : usize = 16;

/// CPU 内存中的 BGRA8888 双缓冲绘制目标。
pub struct ShadowSurface {
    size : Size,
    stride : usize,
    pixels : Vec<u8>,
}

impl ShadowSurface {
    pub fn new(size : Size) -> GuiResult<Self> {
        if size.is_empty() {
            return Err(GuiError::InvalidSurface);
        }
        let stride = (size.width as usize).checked_mul(BYTES_PER_PIXEL)
                                               .ok_or(GuiError::InvalidSurface)?;
        let byte_len = stride.checked_mul(size.height as usize)
                             .ok_or(GuiError::InvalidSurface)?;
        Ok(Self { size,
                  stride,
                  pixels : vec![0; byte_len] })
    }

    pub const fn size(&self) -> Size { self.size }

    pub const fn bounds(&self) -> Rect { Rect::from_size(self.size) }

    pub const fn stride(&self) -> usize { self.stride }

    pub fn pixels(&self) -> &[u8] { &self.pixels }

    pub fn pixels_mut(&mut self) -> &mut [u8] { &mut self.pixels }
}

/// 最多保留若干独立区域；超过容量时退化为一个包围矩形，保证永不丢失更新。
pub struct DirtyRegions {
    surface : Rect,
    regions : [Rect; MAX_DIRTY_REGIONS],
    len : usize,
}

impl DirtyRegions {
    pub fn new(surface : Rect) -> Self {
        Self { surface,
               regions : [Rect::EMPTY; MAX_DIRTY_REGIONS],
               len : 0 }
    }

    pub const fn is_empty(&self) -> bool { self.len == 0 }

    pub fn clear(&mut self) { self.len = 0; }

    pub fn mark_all(&mut self) {
        self.regions[0] = self.surface;
        self.len = usize::from(!self.surface.is_empty());
    }

    pub fn add(&mut self, rect : Rect) {
        let Some(mut clipped) = rect.intersection(self.surface) else {
            return;
        };
        let mut index = 0;
        while index < self.len {
            if touches_or_overlaps(self.regions[index], clipped) {
                clipped = self.regions[index].union(clipped);
                self.len -= 1;
                self.regions[index] = self.regions[self.len];
            } else {
                index += 1;
            }
        }
        if self.len < MAX_DIRTY_REGIONS {
            self.regions[self.len] = clipped;
            self.len += 1;
            return;
        }
        let mut merged = clipped;
        for region in &self.regions[..self.len] {
            merged = merged.union(*region);
        }
        self.regions[0] = merged;
        self.len = 1;
    }

    pub fn take(&mut self) -> Vec<Rect> {
        let result = self.regions[..self.len].to_vec();
        self.len = 0;
        result
    }

    pub fn bounds(&self) -> Option<Rect> {
        self.regions[..self.len].iter().copied().reduce(Rect::union)
    }
}

fn touches_or_overlaps(a : Rect, b : Rect) -> bool {
    a.origin.x <= b.right() && b.origin.x <= a.right() &&
    a.origin.y <= b.bottom() && b.origin.y <= a.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_regions_clip_and_merge() {
        let mut dirty = DirtyRegions::new(Rect::new(0, 0, 100, 100));
        dirty.add(Rect::new(-10, -10, 20, 20));
        dirty.add(Rect::new(9, 9, 10, 10));
        assert_eq!(dirty.bounds(), Some(Rect::new(0, 0, 19, 19)));
        assert_eq!(dirty.take().len(), 1);
        assert!(dirty.is_empty());
    }
}
