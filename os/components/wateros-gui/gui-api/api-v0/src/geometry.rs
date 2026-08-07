//! 采用有符号坐标、无符号尺寸的二维几何类型。

/// 屏幕或窗口坐标。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x : i32,
    pub y : i32,
}

impl Point {
    pub const fn new(x : i32, y : i32) -> Self { Self { x, y } }
}

/// 二维大小。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width : u32,
    pub height : u32,
}

impl Size {
    pub const fn new(width : u32, height : u32) -> Self { Self { width, height } }

    pub const fn is_empty(self) -> bool { self.width == 0 || self.height == 0 }
}

/// 矩形四边的留白大小。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Insets {
    pub left : u32,
    pub top : u32,
    pub right : u32,
    pub bottom : u32,
}

impl Insets {
    pub const fn uniform(value : u32) -> Self {
        Self { left : value,
               top : value,
               right : value,
               bottom : value }
    }
}

/// 左上角与大小表示的半开矩形 `[x, right) × [y, bottom)`。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub origin : Point,
    pub size : Size,
}

impl Rect {
    pub const EMPTY : Self = Self::new(0, 0, 0, 0);

    pub const fn new(x : i32, y : i32, width : u32, height : u32) -> Self {
        Self { origin : Point::new(x, y),
               size : Size::new(width, height) }
    }

    pub const fn from_size(size : Size) -> Self { Self { origin : Point::new(0, 0), size } }

    pub const fn is_empty(self) -> bool { self.size.is_empty() }

    pub fn right(self) -> i32 {
        i64::from(self.origin.x).saturating_add(i64::from(self.size.width)).clamp(i64::from(i32::MIN),
                                                                                  i64::from(i32::MAX)) as i32
    }

    pub fn bottom(self) -> i32 {
        i64::from(self.origin.y).saturating_add(i64::from(self.size.height)).clamp(i64::from(i32::MIN),
                                                                                   i64::from(i32::MAX)) as i32
    }

    pub fn contains(self, point : Point) -> bool {
        !self.is_empty() && point.x >= self.origin.x && point.y >= self.origin.y &&
        point.x < self.right() && point.y < self.bottom()
    }

    pub fn intersects(self, other : Self) -> bool { self.intersection(other).is_some() }

    pub fn intersection(self, other : Self) -> Option<Self> {
        let left = self.origin.x.max(other.origin.x);
        let top = self.origin.y.max(other.origin.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= left || bottom <= top {
            None
        } else {
            Some(Self::new(left, top, (right - left) as u32, (bottom - top) as u32))
        }
    }

    pub fn union(self, other : Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let left = self.origin.x.min(other.origin.x);
        let top = self.origin.y.min(other.origin.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self::new(left, top, (right - left) as u32, (bottom - top) as u32)
    }

    pub fn translated(self, dx : i32, dy : i32) -> Self {
        Self { origin : Point::new(self.origin.x.saturating_add(dx),
                                   self.origin.y.saturating_add(dy)),
               size : self.size }
    }

    pub fn inset(self, insets : Insets) -> Self {
        let horizontal = insets.left.saturating_add(insets.right);
        let vertical = insets.top.saturating_add(insets.bottom);
        Self::new(self.origin.x.saturating_add(insets.left as i32),
                  self.origin.y.saturating_add(insets.top as i32),
                  self.size.width.saturating_sub(horizontal),
                  self.size.height.saturating_sub(vertical))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_and_union_follow_half_open_rules() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 6, 10, 10);
        assert_eq!(a.intersection(b), Some(Rect::new(5, 6, 5, 4)));
        assert_eq!(a.union(b), Rect::new(0, 0, 15, 16));
        assert!(a.contains(Point::new(9, 9)));
        assert!(!a.contains(Point::new(10, 9)));
    }
}
